use super::loads::PhysicalPushoverLoads;
use crate::common::constraint::Reducer;
use crate::common::linear_load::{legacy_member_force, PhysicalLoadLedger};
use sepika_core::dof::DofMap;
use sepika_core::model::{LoadCase, MemberLoadKind, Model};
use sepika_element::behavior::{Ctx, ElementBehavior, LoadedFrameResponse};

pub(super) struct PhysicalStep {
    pub(super) element_nodal: Vec<Vec<(usize, [f64; 6])>>,
    pub(super) support: Vec<[f64; 6]>,
    pub(super) fixed_dofs: Vec<[bool; 6]>,
    pub(super) source: LoadCase,
    pub(super) reference: LoadCase,
}

impl PhysicalStep {
    pub(super) fn recover(
        model: &Model,
        dofmap: &DofMap,
        behaviors: &[Box<dyn ElementBehavior>],
        responses: &[Option<LoadedFrameResponse>],
        total_disp: &[f64],
        source: LoadCase,
        reference: LoadCase,
    ) -> Result<Self, String> {
        let ledger = PhysicalLoadLedger::new(model, &source)
            .map_err(|e| format!("増分解析の物理応答: {e}"))?;
        if behaviors.len() != model.elements.len() || responses.len() != behaviors.len() {
            return Err("増分解析の物理応答: 要素数と応答数が一致しません".into());
        }
        for case in [&source, &reference] {
            for load in &case.nodal {
                if model.node(load.node).is_none() || load.values.iter().any(|v| !v.is_finite()) {
                    return Err(format!(
                        "増分解析の物理応答: 荷重節点 {} の参照または値が不正です",
                        load.node.0
                    ));
                }
            }
            for load in &case.member {
                let error =
                    |reason| format!("要素 ID {} の増分解析物理荷重: {reason}", load.elem.0);
                let elem = model
                    .element(load.elem)
                    .ok_or_else(|| error("荷重対象がありません".into()))?;
                if elem.nodes.len() != 2 {
                    return Err(error("部材荷重の対象は2節点要素に限ります".into()));
                }
                load.validate_direction().map_err(error)?;
                load.validate_extent(model.member_length(elem))
                    .map_err(error)?;
                let finite = match load.kind {
                    MemberLoadKind::Point { p, .. } => p.is_finite(),
                    MemberLoadKind::Distributed { w1, w2, .. } => w1.is_finite() && w2.is_finite(),
                };
                if !finite {
                    return Err(error("荷重強度が非有限です".into()));
                }
            }
        }
        let full = dofmap.including_restrained_node_dofs();
        let mut residual: Vec<_> = ledger.nodal.iter().map(|v| v.map(|x| -x)).collect();
        let mut element_nodal = Vec::with_capacity(behaviors.len());
        for ((elem, behavior), response) in model.elements.iter().zip(behaviors).zip(responses) {
            let error =
                |reason: &str| format!("要素 ID {} の増分解析物理応答: {reason}", elem.id.0);
            let mut nodal = std::collections::BTreeMap::<usize, [f64; 6]>::new();
            if behavior.owns_frame_member_loads() {
                let response = response
                    .as_ref()
                    .ok_or_else(|| error("荷重所有応答がありません"))?;
                for (end, node) in elem.nodes.iter().enumerate() {
                    if node.index() >= model.nodes.len() {
                        return Err(error("節点がありません"));
                    }
                    let values = nodal.entry(node.index()).or_default();
                    values.copy_from_slice(
                        &response.physical_frame_end_force_global[end * 6..end * 6 + 6],
                    );
                }
            } else {
                let force = behavior.internal_force(&Ctx { model });
                let mapping = behavior.global_dofs(&full);
                if force.data.len() != mapping.len() || force.data.iter().any(|v| !v.is_finite()) {
                    return Err(error("確定内力の成分数または値が不正です"));
                }
                for (&active, value) in mapping.iter().zip(&force.data) {
                    if active == usize::MAX {
                        continue;
                    }
                    if active >= full.n_active() {
                        return Err(error("物理自由度対応が範囲外です"));
                    }
                    let global = full.global(active as u32);
                    if full.is_node_dof(global) {
                        nodal.entry(global / 6).or_default()[global % 6] += value;
                    }
                }
                let equivalent = legacy_member_force(model, elem, ledger.member_loads(elem.id));
                for (end, node) in elem.nodes.iter().take(2).enumerate() {
                    for d in 0..6 {
                        nodal.entry(node.index()).or_default()[d] -= equivalent[end * 6 + d];
                    }
                }
            }
            for (&ni, force) in &nodal {
                let values = &mut residual[ni];
                for (value, force) in values.iter_mut().zip(force) {
                    *value += force;
                }
            }
            element_nodal.push(nodal.into_iter().collect());
        }
        let reducer = Reducer::build(model, dofmap);
        let fixed_dofs: Vec<_> = (0..model.nodes.len())
            .map(|node| {
                std::array::from_fn(|d| {
                    dofmap
                        .active(node * 6 + d)
                        .is_none_or(|active| reducer.t_rows[active as usize].is_empty())
                })
            })
            .collect();
        let mut support = vec![[0.; 6]; model.nodes.len()];
        for (ni, values) in support.iter_mut().enumerate() {
            for (d, value) in values.iter_mut().enumerate() {
                *value = match dofmap.active(ni * 6 + d) {
                    Some(active) if !fixed_dofs[ni][d] => {
                        let k = model.nodes[ni]
                            .support_spring
                            .map_or(0., |spring| spring[d]);
                        if k == 0. {
                            0.
                        } else {
                            -k * total_disp.get(active as usize).copied().ok_or_else(|| {
                                "増分解析の物理応答: 支持ばね変位がありません".to_string()
                            })?
                        }
                    }
                    _ => residual[ni][d],
                };
                if !value.is_finite() {
                    return Err("増分解析の物理応答: 支持反力が非有限です".into());
                }
            }
        }
        Ok(Self {
            element_nodal,
            support,
            fixed_dofs,
            source,
            reference,
        })
    }

    pub(super) fn support_above(&self, model: &Model, z: f64, d: usize) -> f64 {
        model
            .nodes
            .iter()
            .filter(|n| n.coord[2] > z)
            .map(|n| self.support[n.id.index()][d])
            .sum()
    }
}

pub(super) fn current_case(loads: &PhysicalPushoverLoads, mu: f64, lambda: f64) -> LoadCase {
    let mut source = loads.gravity_case();
    for load in &mut source.nodal {
        for value in &mut load.values {
            *value *= mu;
        }
    }
    for load in &mut source.member {
        scale_member(load, mu);
    }
    source
        .nodal
        .extend(loads.reference.nodal.iter().cloned().map(|mut load| {
            for value in &mut load.values {
                *value *= lambda;
            }
            load
        }));
    source
        .member
        .extend(loads.reference.member.iter().cloned().map(|mut load| {
            scale_member(&mut load, lambda);
            load
        }));
    source
}

fn scale_member(load: &mut sepika_core::model::MemberLoad, factor: f64) {
    match &mut load.kind {
        MemberLoadKind::Point { p, .. } => *p *= factor,
        MemberLoadKind::Distributed { w1, w2, .. } => {
            *w1 *= factor;
            *w2 *= factor;
        }
    }
}

pub(super) fn load_above(
    model: &Model,
    source: &LoadCase,
    z: f64,
    d: usize,
) -> Result<f64, String> {
    let mut sum = 0.;
    for load in &source.nodal {
        let node = model
            .node(load.node)
            .ok_or_else(|| "増分解析: 荷重節点がありません".to_string())?;
        if node.coord[2] > z {
            sum += load.values[d];
        }
    }
    for load in &source.member {
        let elem = model
            .element(load.elem)
            .ok_or_else(|| format!("要素 ID {} の増分解析: 荷重対象がありません", load.elem.0))?;
        if elem.nodes.len() != 2 {
            return Err(format!(
                "要素 ID {} の増分解析: 部材荷重の節点数が不正です",
                elem.id.0
            ));
        }
        let pi = model
            .node(elem.nodes[0])
            .ok_or("増分解析: 部材荷重節点がありません")?;
        let pj = model
            .node(elem.nodes[1])
            .ok_or("増分解析: 部材荷重節点がありません")?;
        let length = model.member_length(elem);
        load.validate_direction()?;
        load.validate_extent(length)?;
        let dz = pj.coord[2] - pi.coord[2];
        let x = if dz == 0. {
            0.
        } else {
            (z - pi.coord[2]) / dz * length
        };
        let magnitude = match load.kind {
            MemberLoadKind::Point { a, p } => {
                let above = if dz > 0. {
                    a >= x
                } else if dz < 0. {
                    a < x
                } else {
                    pi.coord[2] > z
                };
                if above {
                    p
                } else {
                    0.
                }
            }
            MemberLoadKind::Distributed { a, b, w1, w2 } => {
                let (lo, hi) = if dz > 0. {
                    (a.max(x).min(b), b)
                } else if dz < 0. {
                    (a, b.min(x).max(a))
                } else if pi.coord[2] > z {
                    (a, b)
                } else {
                    (a, a)
                };
                let wlo = w1 + (w2 - w1) * ((lo - a) / (b - a));
                let whi = w1 + (w2 - w1) * ((hi - a) / (b - a));
                (hi - lo) * (wlo + whi) * 0.5
            }
        };
        let norm = sepika_core::geom::vec3::norm(load.dir);
        sum += magnitude * load.dir[d] / norm;
    }
    if !sum.is_finite() {
        return Err("増分解析: 切断片外力が非有限です".into());
    }
    Ok(sum)
}
