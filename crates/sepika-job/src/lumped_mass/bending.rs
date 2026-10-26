use super::*;
use sepika_design_jp::secondary::story_columns::checked_story_columns;
use sepika_solver::dynamic::lumped_mass::{bending_drifts, fit_story_trilinear, StoryBending};

fn bad(layer: &Layer, reason: impl std::fmt::Display) -> JobError {
    JobError::InvalidInput(format!("階 {}: {reason}", layer.name))
}

fn axial_at(
    model: &Model,
    res: &StaticOnce,
    id: sepika_core::ids::ElemId,
    upper: bool,
) -> JobResult<f64> {
    let elem = model
        .element(id)
        .ok_or_else(|| JobError::InvalidInput(format!("柱 {} がありません", id.0)))?;
    let p0 = model.nodes[elem.nodes[0].index()].coord;
    let p1 = model.nodes[elem.nodes[1].index()].coord;
    let frame = LocalFrame::from_nodes(p0, p1, elem.local_axis.ref_vector)
        .map_err(|e| JobError::InvalidInput(e.into()))?;
    let forward = p1[2] > p0[2];
    let force = res
        .member_forces
        .iter()
        .find(|(key, _)| *key == id)
        .and_then(|(_, f)| {
            if upper == forward {
                f.at.last()
            } else {
                f.at.first()
            }
        })
        .ok_or_else(|| JobError::InvalidInput(format!("柱 {} の軸力がありません", id.0)))?
        .1[0];
    let n = force * frame.rot[0][2].abs();
    if !n.is_finite() {
        return Err(JobError::InvalidInput(format!(
            "柱 {} の鉛直軸力が不正です: {}",
            id.0, n
        )));
    }
    Ok(n)
}

fn ei_values(model: &Model, res: &StaticOnce, dir: SeismicDir) -> JobResult<Vec<f64>> {
    let mut result = Vec::new();
    for layer in model.layers() {
        let cols = checked_story_columns(model, layer.top).map_err(|e| bad(&layer, e))?;
        let covered: std::collections::HashSet<_> = cols
            .iter()
            .flat_map(|c| c.segments.iter().copied())
            .collect();
        for element in layer_elements(model, &layer)? {
            if !model
                .element_section(element)
                .is_some_and(|s| s.frame_use == Some(sepika_core::model::FrameSectionUse::Column))
            {
                continue;
            }
            let a = model.nodes[element.nodes[0].index()].coord;
            let b = model.nodes[element.nodes[1].index()].coord;
            if a[0] != b[0] || a[1] != b[1] {
                return Err(bad(
                    &layer,
                    format!(
                        "傾斜柱 {} は鉛直軸変形を柱軸伸縮として代表できないため対象外です",
                        element.id.0
                    ),
                ));
            }
            if !covered.contains(&element.id) {
                return Err(bad(&layer,format!("柱 {} は上下階SLを結ぶ非分岐の柱として追跡できません（下端階未到達・中間分岐・複数階横断を確認してください）",element.id.0)));
            }
        }
        if cols.is_empty() {
            return Err(bad(&layer, "曲げ剛性を算定する柱がありません"));
        }
        let axis = if dir == SeismicDir::X { 0 } else { 1 };
        let sign = if dir == SeismicDir::X { -1.0 } else { 1.0 };
        let origin = cols
            .iter()
            .map(|c| model.nodes[c.top.index()].coord[axis])
            .sum::<f64>()
            / cols.len() as f64;
        let mut mb = 0.0;
        let mut mt = 0.0;
        let mut work = 0.0;
        for c in cols {
            for id in &c.segments {
                let e = model.element(*id).unwrap();
                let a = model.nodes[e.nodes[0].index()].coord;
                let b = model.nodes[e.nodes[1].index()].coord;
                if a[0] != b[0] || a[1] != b[1] {
                    return Err(bad(&layer, format!("傾斜柱 {} は鉛直軸変形を柱軸伸縮として代表できないため曲げせん断型の対象外です", id.0)));
                }
            }
            let ub = res
                .disp
                .get(c.bottom.index())
                .ok_or_else(|| bad(&layer, format!("柱 {} 下端変位がありません", c.top_elem.0)))?;
            let ut = res
                .disp
                .get(c.top.index())
                .ok_or_else(|| bad(&layer, format!("柱 {} 上端変位がありません", c.top_elem.0)))?;
            let nt = axial_at(model, res, c.segments[0], true).map_err(|e| bad(&layer, e))?;
            let nb = axial_at(model, res, *c.segments.last().unwrap(), false)
                .map_err(|e| bad(&layer, e))?;
            let dz = ut[2] - ub[2];
            if !dz.is_finite() {
                return Err(bad(
                    &layer,
                    format!("柱 {} の鉛直軸変形が不正です", c.top_elem.0),
                ));
            }
            mt += sign * nt * (model.nodes[c.top.index()].coord[axis] - origin);
            mb += sign * nb * (model.nodes[c.bottom.index()].coord[axis] - origin);
            work += 0.5 * (nb + nt) * dz;
        }
        let moment = 0.5 * (mb + mt);
        let theta = work / moment;
        let h = model.stories[layer.top.index()]
            .sl_mm()
            .zip(model.stories[layer.bottom.index()].sl_mm())
            .map(|(t, b)| t - b)
            .ok_or_else(|| bad(&layer, "構造階高が未設定です"))?;
        let ei = h * moment / theta;
        if !h.is_finite()
            || h <= 0.0
            || !moment.is_finite()
            || moment == 0.0
            || !work.is_finite()
            || work <= 0.0
            || !ei.is_finite()
            || ei <= 0.0
        {
            return Err(bad(&layer,format!("{:?}方向の曲げ剛性を算定できません: h={} mm, Mb={} Nmm, Mt={} Nmm, ΣNΔuz={} Nmm, Δθ={} rad, EI={} Nmm²",dir,h,mb,mt,work,theta,ei)));
        }
        result.push(ei);
    }
    Ok(result)
}

fn scope_location(model: &Model, node: sepika_core::ids::NodeId) -> String {
    let node = &model.nodes[node.index()];
    let name = node
        .story
        .and_then(|id| model.stories.get(id.index()))
        .map(|s| s.name.clone())
        .or_else(|| {
            model
                .layers()
                .iter()
                .find(|layer| {
                    layer.bottom_sl_mm <= node.coord[2] && node.coord[2] <= layer.top_sl_mm
                })
                .map(|layer| layer.name.clone())
        })
        .unwrap_or_else(|| "未所属".into());
    format!("階 {}（高さ {} mm）", name, node.coord[2])
}

fn validate_scope(inp: &LumpedMassBuildInput<'_>) -> JobResult<()> {
    use sepika_core::dof::Dof;
    use sepika_core::model::ElementKind;
    let model = inp.model;
    let mut structural = sepika_core::dof::structural_nodes(model);
    for constraint in &model.constraints {
        use sepika_core::model::Constraint;
        match constraint {
            Constraint::RigidDiaphragm { master, slaves, .. }
            | Constraint::RigidLink { master, slaves, .. } => {
                structural[master.index()] = true;
                for node in slaves {
                    structural[node.index()] = true;
                }
            }
            Constraint::Mpc { master, terms } => {
                structural[master.index()] = true;
                for (node, _, _) in terms {
                    structural[node.index()] = true;
                }
            }
        }
    }
    let base_z = model.layers().first().map(|layer| layer.bottom_sl_mm);
    let horizontal = if inp.dim == StickDim::Spatial {
        &[Dof::Ux, Dof::Uy][..]
    } else if inp.dir == SeismicDir::X {
        &[Dof::Ux][..]
    } else {
        &[Dof::Uy][..]
    };
    for node in &model.nodes {
        if !structural[node.id.index()] {
            continue;
        }
        if let Some(spring) = node.support_spring {
            let dofs = [Dof::Ux, Dof::Uy, Dof::Uz, Dof::Rx, Dof::Ry, Dof::Rz];
            if let Some((dof, value)) = dofs
                .into_iter()
                .zip(spring)
                .find(|(dof, value)| !node.restraint.is_fixed(*dof) && *value != 0.0)
            {
                return Err(JobError::InvalidInput(format!(
                    "{}: 節点 {} の活動する支持ばね（{:?}={}）は基部固定の曲げせん断型の対象外です",
                    scope_location(model, node.id),
                    node.id.0,
                    dof,
                    value
                )));
            }
        }
        if base_z.is_some_and(|z| node.coord[2] > z)
            && horizontal.iter().any(|dof| node.restraint.is_fixed(*dof))
        {
            return Err(JobError::InvalidInput(format!("{}: 節点 {} は基部以外で解析対象の水平方向が固定されているため、基部固定の曲げせん断型の対象外です",scope_location(model,node.id),node.id.0)));
        }
    }
    for element in &model.elements {
        if element.kind == ElementKind::PanelZone {
            continue;
        }
        if matches!(element.kind, ElementKind::Beam | ElementKind::Brace { .. })
            && element.nodes.len() == 2
        {
            continue;
        }
        let kind = match element.kind {
            ElementKind::NodalSpring => "節点ばね",
            ElementKind::Isolator => "免震部材",
            ElementKind::Damper => "制振ダンパー",
            ElementKind::Wall => "壁要素",
            ElementKind::Shell => "シェル要素",
            ElementKind::Fiber => "ファイバー要素",
            ElementKind::MultiSpring => "マルチスプリング要素",
            _ => "接続節点数が不正な骨組要素",
        };
        let location = element
            .nodes
            .first()
            .map(|node| scope_location(model, *node))
            .unwrap_or_else(|| "階未所属".into());
        return Err(JobError::InvalidInput(format!(
            "{}: 部材 {} の{}は柱・梁・ブレース骨組の曲げせん断型の対象外です",
            location, element.id.0, kind
        )));
    }
    Ok(())
}

fn layer_elements<'a>(
    model: &'a Model,
    layer: &Layer,
) -> JobResult<Vec<&'a sepika_core::model::ElementData>> {
    let bottom = model.stories[layer.bottom.index()]
        .sl_mm()
        .ok_or_else(|| bad(layer, "下端SLが未設定です"))?;
    let top = model.stories[layer.top.index()]
        .sl_mm()
        .ok_or_else(|| bad(layer, "上端SLが未設定です"))?;
    let cut = 0.5 * (bottom + top);
    let mut elements = Vec::new();
    for e in &model.elements {
        if e.kind == sepika_core::model::ElementKind::PanelZone {
            continue;
        }
        let (zmin, zmax) =
            e.nodes
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), id| {
                    let z = model.nodes[id.index()].coord[2];
                    (lo.min(z), hi.max(z))
                });
        if !(zmin <= cut && cut < zmax) {
            continue;
        }
        if e.nodes.len() != 2
            || !matches!(
                e.kind,
                sepika_core::model::ElementKind::Beam
                    | sepika_core::model::ElementKind::Brace { .. }
            )
        {
            continue;
        }
        elements.push(e);
    }
    Ok(elements)
}

fn signed_shears(model: &Model, res: &StaticOnce, dir: SeismicDir) -> JobResult<Vec<f64>> {
    let axis = usize::from(dir == SeismicDir::Y);
    let mut result = Vec::new();
    for layer in model.layers() {
        let mut q = 0.0;
        for e in layer_elements(model, &layer)? {
            let p0 = model.nodes[e.nodes[0].index()].coord;
            let p1 = model.nodes[e.nodes[1].index()].coord;
            let frame = LocalFrame::from_nodes(p0, p1, e.local_axis.ref_vector)
                .map_err(|e| bad(&layer, e))?;
            let local = res
                .member_forces
                .iter()
                .find(|(id, _)| *id == e.id)
                .and_then(|(_, f)| f.at.first())
                .ok_or_else(|| bad(&layer, format!("部材 {} の層せん断力がありません", e.id.0)))?
                .1;
            q += frame.rot[0][2].signum()
                * (local[0] * frame.rot[0][axis]
                    - local[1] * frame.rot[1][axis]
                    - local[2] * frame.rot[2][axis]);
        }
        if !q.is_finite() {
            return Err(bad(&layer, format!("層せん断力 Q={} が不正です", q)));
        }
        result.push(q);
    }
    Ok(result)
}

fn step_drifts(
    inp: &LumpedMassBuildInput<'_>,
    dir: SeismicDir,
    step: &sepika_solver::nonlinear::pushover::PushoverStep,
    point: &sepika_solver::nonlinear::pushover::CapacityPoint,
    base: &LumpedMassModel,
) -> JobResult<Vec<f64>> {
    if inp.dim == StickDim::Planar {
        return Ok(point.story_drift.clone());
    }
    if step.floor_disp.len() != inp.model.stories.len() {
        return Err(JobError::InvalidInput("3次元曲げせん断型に必要な増分解析の各階変位・ねじれ回転が不足しています。増分解析を再実行してください".into()));
    }
    let axis = usize::from(dir == SeismicDir::Y);
    inp.model
        .layers()
        .iter()
        .enumerate()
        .map(|(i, layer)| {
            let xy = base.spatial[i].rigidity_xy;
            let displacement = |sid: sepika_core::ids::StoryId| -> JobResult<f64> {
                let story = &inp.model.stories[sid.index()];
                let nid = story_reference_node(inp.model, story).ok_or_else(|| {
                    bad(
                        layer,
                        format!("階 {} の増分応答の代表節点がありません", story.name),
                    )
                })?;
                let p = inp.model.nodes[nid.index()].coord;
                let u = step.floor_disp[sid.index()];
                if u.iter().any(|v| !v.is_finite()) {
                    return Err(bad(
                        layer,
                        format!(
                            "階 {} の増分変位・ねじれ回転が非有限です: {:?}",
                            story.name, u
                        ),
                    ));
                }
                Ok(if axis == 0 {
                    u[0] - (xy[1] - p[1]) * u[2]
                } else {
                    u[1] + (xy[0] - p[0]) * u[2]
                })
            };
            Ok(displacement(layer.top)? - displacement(layer.bottom)?)
        })
        .collect()
}

fn skeletons(
    inp: &LumpedMassBuildInput<'_>,
    dir: SeismicDir,
    ei: &[f64],
    height: &[f64],
    base: &LumpedMassModel,
) -> JobResult<Vec<StoryTrilinear>> {
    let layers = inp.model.layers();
    let res = static_of(inp, dir)?;
    if !inp.nonlinear {
        let q = signed_shears(inp.model, res, dir)?;
        let db =
            bending_drifts(height, ei, &q).map_err(|e| JobError::InvalidInput(e.to_string()))?;
        let mut sk = Vec::new();
        for (i, layer) in layers.iter().enumerate() {
            let d = if inp.dim == StickDim::Spatial {
                story_drift_at(inp.model, res, dir, layer, base.spatial[i].rigidity_xy)
            } else {
                story_drifts(inp.model, res, dir)[i]
            };
            let ds = d - db[i];
            let k = q[i] / ds;
            if !k.is_finite() || k <= 0.0 || q[i] * ds <= 0.0 {
                return Err(bad(layer,format!("正のせん断剛性を算定できません: Q={} N, δ={} mm, δb={} mm, δs={} mm, Ks={} N/mm",q[i],d,db[i],ds,k)));
            }
            sk.push(StoryTrilinear::elastic(k));
        }
        return Ok(sk);
    }
    let po = pushover_of(inp, dir)?;
    if po.steps.len() != po.capacity_curve.len() || po.steps.is_empty() {
        return Err(JobError::InvalidInput(
            "増分解析の確定ステップと性能曲線が不足・不整合です".into(),
        ));
    }
    let baseline = po.steps.iter().rposition(|s| s.load_factor == 0.0);
    let zeros = vec![0.0; layers.len()];
    let (d0, q0) = if let Some(i) = baseline {
        (
            step_drifts(inp, dir, &po.steps[i], &po.capacity_curve[i], base)?,
            &po.capacity_curve[i].story_shear,
        )
    } else {
        (zeros.clone(), &zeros)
    };
    if d0.len() != layers.len() || q0.len() != layers.len() {
        return Err(JobError::InvalidInput(
            "増分解析の水平力ゼロ基準点の層数が一致しません".into(),
        ));
    }
    let mut curves = vec![Vec::new(); layers.len()];
    for (step, cp) in po.steps.iter().zip(&po.capacity_curve) {
        if step.load_factor == 0.0 {
            continue;
        }
        if cp.story_drift.len() != layers.len() || cp.story_shear.len() != layers.len() {
            return Err(JobError::InvalidInput(
                "増分解析の層変位・せん断力が不足しています".into(),
            ));
        }
        let q: Vec<_> = cp.story_shear.iter().zip(q0).map(|(q, b)| q - b).collect();
        let db =
            bending_drifts(height, ei, &q).map_err(|e| JobError::InvalidInput(e.to_string()))?;
        let sign = step.load_factor.signum();
        let drift = step_drifts(inp, dir, step, cp, base)?;
        for i in 0..layers.len() {
            let ds = sign * (drift[i] - d0[i] - db[i]);
            let qi = sign * q[i];
            if !ds.is_finite()
                || !qi.is_finite()
                || ds <= 0.0
                || qi <= 0.0
                || curves[i].last().is_some_and(|&(d, _)| ds <= d)
            {
                return Err(bad(
                    &layers[i],
                    format!(
                        "有効な Q−δs 骨格を作れません: λ={}, Q={} N, δs={} mm",
                        step.load_factor, qi, ds
                    ),
                ));
            }
            curves[i].push((ds, qi));
        }
    }
    curves
        .iter()
        .enumerate()
        .map(|(i, curve)| {
            if curve.len() < 3 {
                return Err(bad(
                    &layers[i],
                    "せん断骨格に必要な確定ステップが3点未満です",
                ));
            }
            let sk = fit_story_trilinear(curve, inp.secant_ratio);
            if !sk.is_valid_spring() {
                return Err(bad(
                    &layers[i],
                    format!("トリリニア縮約後のせん断骨格が不正です: {:?}", sk),
                ));
            }
            Ok(sk)
        })
        .collect()
}

pub(super) fn build(inp: LumpedMassBuildInput<'_>) -> JobResult<LumpedMassModel> {
    validate_scope(&inp)?;
    let selected = static_of(&inp, inp.dir)?;
    let ex = if inp.dim == StickDim::Spatial {
        static_of(&inp, SeismicDir::X)?
    } else {
        selected
    };
    let ey = if inp.dim == StickDim::Spatial {
        static_of(&inp, SeismicDir::Y)?
    } else {
        selected
    };
    let xi = ei_values(
        inp.model,
        ex,
        if inp.dim == StickDim::Spatial {
            SeismicDir::X
        } else {
            inp.dir
        },
    )?;
    let yi = if inp.dim == StickDim::Spatial {
        ei_values(inp.model, ey, SeismicDir::Y)?
    } else {
        xi.clone()
    };
    let mut lm = match inp.dim {
        StickDim::Planar => {
            let stories = inp
                .model
                .layers()
                .iter()
                .map(|layer| {
                    Ok(StoryStick {
                        story: layer.bottom,
                        mass: layer_mass(layer)?,
                        height: layer.height,
                        skeleton: StoryTrilinear::elastic(1.0),
                    })
                })
                .collect::<JobResult<Vec<_>>>()?;
            let mut model =
                LumpedMassModel::from_stories(LumpedMassType::EquivalentBendingShear, stories);
            model.dir = inp.dir;
            model
        }
        StickDim::Spatial => build_spatial(LumpedMassBuildInput {
            model_type: LumpedMassType::EquivalentShear,
            nonlinear: false,
            source: LumpedStiffnessSource::StoryQd,
            ..inp
        })?,
    };
    let layers = inp.model.layers();
    let mut heights = Vec::new();
    for (i, layer) in layers.iter().enumerate() {
        let h = inp.model.stories[layer.top.index()].sl_mm().unwrap()
            - inp.model.stories[layer.bottom.index()].sl_mm().unwrap();
        lm.stories[i].height = h;
        heights.push(h);
        let dm = layer
            .dynamic_mass
            .ok_or_else(|| bad(layer, "動的物理質量がありません"))?;
        let (ei_x, ei_y) = if inp.dim == StickDim::Spatial {
            (yi[i], xi[i])
        } else if inp.dir == SeismicDir::X {
            (0.0, xi[i])
        } else {
            (xi[i], 0.0)
        };
        lm.bending.push(StoryBending {
            ei_x,
            ei_y,
            inertia: dm.horizontal_inertia_t_mm2,
        });
    }
    let skx = skeletons(
        &inp,
        if inp.dim == StickDim::Spatial {
            SeismicDir::X
        } else {
            inp.dir
        },
        &xi,
        &heights,
        &lm,
    )?;
    let sky = if inp.dim == StickDim::Spatial {
        skeletons(&inp, SeismicDir::Y, &yi, &heights, &lm)?
    } else {
        skx.clone()
    };
    for i in 0..layers.len() {
        lm.stories[i].skeleton = if inp.dir == SeismicDir::X || inp.dim == StickDim::Planar {
            skx[i]
        } else {
            sky[i]
        };
        if inp.dim == StickDim::Spatial {
            lm.spatial[i].skeleton_x = skx[i];
            lm.spatial[i].skeleton_y = sky[i];
            lm.spatial[i].k1_x = skx[i].k1;
            lm.spatial[i].k1_y = sky[i].k1;
        }
    }
    lm.model_type = LumpedMassType::EquivalentBendingShear;
    lm.nonlinear = inp.nonlinear;
    Ok(lm)
}
#[cfg(test)]
mod tests {
    use super::*;
    use sepika_core::ids::{ElemId, NodeId, StoryId};
    use sepika_core::model::Constraint;
    use sepika_element::frame::beam::MemberForces;

    fn fixture(dir: SeismicDir, reverse: bool, split: bool) -> (Model, StaticOnce) {
        let mut model = super::super::tests::two_story_column_model();
        let node = model.nodes[0].clone();
        let element = model.elements[0].clone();
        model.nodes.clear();
        model.elements.clear();
        model.constraints.clear();
        let xy = [
            [-1000.0, -2000.0],
            [1000.0, -2000.0],
            [-1000.0, 2000.0],
            [1000.0, 2000.0],
        ];
        for level in 0..3 {
            let ids: Vec<_> = (0..4).map(|i| NodeId((level * 4 + i) as u32)).collect();
            for (k, &[x, y]) in xy.iter().enumerate() {
                let mut n = node.clone();
                n.id = ids[k];
                n.coord = [x, y, level as f64 * 3000.0];
                n.story = Some(StoryId(level as u32));
                n.restraint = if level == 0 {
                    sepika_core::dof::Dof6Mask::FIXED
                } else {
                    sepika_core::dof::Dof6Mask::FREE
                };
                model.nodes.push(n);
            }
            model.stories[level].node_ids = ids.clone();
            let dm = model.stories[level].dynamic_mass.as_mut().unwrap();
            dm.center_xy_mm = [0.0, 0.0];
            dm.horizontal_inertia_t_mm2 = [4e7, 1e7, 0.0];
            dm.inertia_t_mm2 = 5e7;
            model.constraints.push(Constraint::RigidDiaphragm {
                story: StoryId(level as u32),
                master: ids[0],
                slaves: ids[1..].to_vec(),
                weight: None,
                ci_override: None,
            });
        }
        for layer in 0..2 {
            for (k, point) in xy.iter().enumerate() {
                let bottom = NodeId((layer * 4 + k) as u32);
                let top = NodeId(((layer + 1) * 4 + k) as u32);
                let segments = if split {
                    let mut n = node.clone();
                    n.id = NodeId(model.nodes.len() as u32);
                    n.coord = [point[0], point[1], layer as f64 * 3000.0 + 1500.0];
                    n.story = Some(StoryId(layer as u32));
                    n.restraint = sepika_core::dof::Dof6Mask::FREE;
                    let mid = n.id;
                    model.nodes.push(n);
                    vec![[bottom, mid], [mid, top]]
                } else {
                    vec![[bottom, top]]
                };
                for mut pair in segments {
                    if reverse {
                        pair.reverse();
                    }
                    let mut e = element.clone();
                    e.id = ElemId(model.elements.len() as u32);
                    e.nodes = pair.into_iter().collect();
                    model.elements.push(e);
                }
            }
        }
        let axis = usize::from(dir == SeismicDir::Y);
        let ei = if axis == 0 { 1.2e14 } else { 4.8e14 };
        let db = bending_drifts(&[3000.0; 2], &[ei; 2], &[2000.0, 1000.0]).unwrap();
        let mut disp = vec![[0.0; 6]; model.nodes.len()];
        for n in &model.nodes {
            let level = n.coord[2] / 3000.0;
            let moments = [6e6, 3e6];
            let mut theta = 0.0;
            let full = level.floor() as usize;
            for &m in moments.iter().take(full) {
                theta += 3000.0 * m / ei;
            }
            if full < 2 {
                theta += (level - full as f64) * 3000.0 * moments[full] / ei;
            }
            disp[n.id.index()][2] = -theta * n.coord[axis];
            if level == 1.0 {
                disp[n.id.index()][axis] = db[0] + 0.2;
            }
            if level == 2.0 {
                disp[n.id.index()][axis] = db[0] + db[1] + 0.3;
            }
        }
        let mut member_forces = Vec::new();
        for e in &model.elements {
            let p0 = model.nodes[e.nodes[0].index()].coord;
            let p1 = model.nodes[e.nodes[1].index()].coord;
            let layer = ((p0[2] + p1[2]) / 2.0 / 3000.0) as usize;
            let n = -[6e6, 3e6][layer] * p0[axis] / if axis == 0 { 4e6 } else { 16e6 };
            let frame = LocalFrame::from_nodes(p0, p1, e.local_axis.ref_vector).unwrap();
            let q = [2000.0, 1000.0][layer] / 4.0;
            let local = [
                n,
                -q * frame.rot[0][2].signum() * frame.rot[1][axis],
                -q * frame.rot[0][2].signum() * frame.rot[2][axis],
                0.0,
                0.0,
                0.0,
            ];
            member_forces.push((
                e.id,
                MemberForces {
                    at: vec![(0.0, local), (1.0, local)],
                },
            ));
        }
        (
            model,
            StaticOnce {
                disp,
                member_forces,
                panel_moments: Vec::new(),
            },
        )
    }
    fn input<'a>(m: &'a Model, r: &'a StaticOnce, dir: SeismicDir) -> LumpedMassBuildInput<'a> {
        LumpedMassBuildInput {
            model_type: LumpedMassType::EquivalentBendingShear,
            model: m,
            dim: StickDim::Planar,
            source: LumpedStiffnessSource::StoryQd,
            dir,
            nonlinear: false,
            secant_ratio: 0.75,
            res_x: Some(r),
            res_y: Some(r),
            po_x: None,
            po_y: None,
        }
    }
    fn near(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9 * b.abs().max(1.0), "{a} != {b}");
    }

    #[test]
    fn column_axial_work_recovers_ei_axes_units_and_split_columns() {
        for dir in [SeismicDir::X, SeismicDir::Y] {
            for reverse in [false, true] {
                for split in [false, true] {
                    let (model, res) = fixture(dir, reverse, split);
                    let expected = if dir == SeismicDir::X { 1.2e14 } else { 4.8e14 };
                    for ei in ei_values(&model, &res, dir).unwrap() {
                        near(ei, expected);
                    }
                    assert_eq!(
                        signed_shears(&model, &res, dir).unwrap(),
                        vec![2000.0, 1000.0]
                    );
                    let lm = build(input(&model, &res, dir)).unwrap();
                    near(lm.stories[0].skeleton.k1, 10000.0);
                    near(lm.stories[1].skeleton.k1, 10000.0);
                    let mut opposite = res.clone();
                    for u in &mut opposite.disp {
                        for v in u {
                            *v = -*v;
                        }
                    }
                    for (_, f) in &mut opposite.member_forces {
                        for (_, v) in &mut f.at {
                            for x in v {
                                *x = -*x;
                            }
                        }
                    }
                    for ei in ei_values(&model, &opposite, dir).unwrap() {
                        near(ei, expected);
                    }
                    let lm2 = build(input(&model, &opposite, dir)).unwrap();
                    near(lm2.stories[0].skeleton.k1, 10000.0);
                }
            }
        }
    }

    #[test]
    fn nonlinear_backbone_subtracts_baseline_and_its_own_load_pattern() {
        use sepika_solver::nonlinear::pushover::{CapacityPoint, PushoverStep};
        let (model, res) = fixture(SeismicDir::X, false, false);
        let mut po = super::super::tests::empty_pushover();
        for (step, (factor, ds)) in [(0.0, 0.0), (1.0, 0.1), (2.0, 0.2), (3.0, 0.4), (4.0, 0.8)]
            .into_iter()
            .enumerate()
        {
            let q = vec![1000.0 * factor, 750.0 * factor];
            let db = bending_drifts(&[3000.0; 2], &[1.2e14; 2], &q).unwrap();
            let d = vec![2.0 + ds + db[0], 3.0 + ds + db[1]];
            po.capacity_curve.push(CapacityPoint {
                step: step as u32,
                roof_disp: d[0] + d[1],
                base_shear: q[0] + 100.0,
                story_shear: vec![q[0] + 100.0, q[1] + 200.0],
                story_drift: d.clone(),
            });
            po.steps.push(PushoverStep {
                floor_disp: Vec::new(),
                load_factor: factor,
                top_disp: d[0] + d[1],
                base_shear: q[0] + 100.0,
                story_drifts: d,
            });
        }
        let mut inp = input(&model, &res, SeismicDir::X);
        inp.nonlinear = true;
        inp.po_x = Some(&po);
        let lm = build(inp).unwrap();
        near(lm.stories[0].skeleton.k1, 10000.0);
        near(lm.stories[1].skeleton.k1, 7500.0);
        near(lm.stories[0].skeleton.d3, 0.8);
        near(lm.stories[1].skeleton.q3, 3000.0);
        assert!(sepika_solver::dynamic::lumped_mass::lumped_mass_eigen(&lm, 2).is_ok());
        let r = sepika_solver::dynamic::lumped_mass::bending_shear_time_history(
            &lm,
            &[0.0, 1.0, -1.0, 0.0],
            0.01,
            0.02,
        )
        .unwrap();
        assert_eq!(r.time.len(), 4);
    }

    #[test]
    fn invalid_axial_results_and_nonpositive_shear_deformation_report_story() {
        let (model, mut res) = fixture(SeismicDir::X, false, false);
        res.member_forces.clear();
        let err = build(input(&model, &res, SeismicDir::X))
            .unwrap_err()
            .to_string();
        assert!(err.contains("柱") && err.contains("軸力"), "{err}");
        let (model, mut res) = fixture(SeismicDir::X, false, false);
        for u in &mut res.disp {
            u[0] = 0.0;
        }
        let err = build(input(&model, &res, SeismicDir::X))
            .unwrap_err()
            .to_string();
        assert!(err.contains("階") && err.contains("δs"), "{err}");
    }
    #[test]
    fn inclined_columns_are_rejected_before_axial_work_is_used() {
        let (mut model, res) = fixture(SeismicDir::X, false, false);
        model.nodes[4].coord[0] += 1.0;
        let error = ei_values(&model, &res, SeismicDir::X)
            .unwrap_err()
            .to_string();
        assert!(error.contains("傾斜柱") && error.contains("階"), "{error}");
    }
    #[test]
    fn unsupported_vertical_wall_is_not_omitted_from_story_force() {
        let (mut model, res) = fixture(SeismicDir::X, false, false);
        let mut wall = model.elements[0].clone();
        wall.id = ElemId(model.elements.len() as u32);
        wall.kind = sepika_core::model::ElementKind::Wall;
        wall.nodes = [NodeId(0), NodeId(1), NodeId(4), NodeId(5)]
            .into_iter()
            .collect();
        model.elements.push(wall);
        let error = validate_scope(&input(&model, &res, SeismicDir::X))
            .unwrap_err()
            .to_string();
        assert!(error.contains("階") && error.contains("対象外"), "{error}");
    }
    #[test]
    fn unsupported_wall_is_rejected_in_every_dimension_and_linearity() {
        let (mut model, res) = fixture(SeismicDir::X, false, false);
        let mut wall = model.elements[0].clone();
        wall.id = ElemId(model.elements.len() as u32);
        wall.kind = sepika_core::model::ElementKind::Wall;
        wall.nodes = [NodeId(0), NodeId(1), NodeId(4), NodeId(5)]
            .into_iter()
            .collect();
        model.elements.push(wall);
        for dim in [StickDim::Planar, StickDim::Spatial] {
            for nonlinear in [false, true] {
                let mut inp = input(&model, &res, SeismicDir::X);
                inp.dim = dim;
                inp.nonlinear = nonlinear;
                let error = build(inp).unwrap_err().to_string();
                assert!(
                    error.contains("階") && error.contains("対象外"),
                    "{dim:?}/{nonlinear}: {error}"
                );
            }
        }
    }
    fn spatial_pushover(model: &Model, dir: SeismicDir) -> PushoverResult {
        use sepika_solver::nonlinear::pushover::{CapacityPoint, PushoverStep};
        let mut po = super::super::tests::empty_pushover();
        let ei = if dir == SeismicDir::X { 1.2e14 } else { 4.8e14 };
        for (index, (factor, ds)) in [(0.0, 0.0), (1.0, 0.1), (2.0, 0.2), (3.0, 0.4), (4.0, 0.8)]
            .into_iter()
            .enumerate()
        {
            let q = [1000.0 * factor, 750.0 * factor];
            let db = bending_drifts(&[3000.0; 2], &[ei; 2], &q).unwrap();
            let floor_u = [0.0, 2.0 + ds + db[0], 5.0 + 2.0 * ds + db[0] + db[1]];
            let mut floor_disp = Vec::new();
            for (i, story) in model.stories.iter().enumerate() {
                let node = story_reference_node(model, story).unwrap();
                let xy = model.nodes[node.index()].coord;
                let theta = [0.0, 0.0001 + 0.0003 * factor, 0.0002 - 0.0002 * factor][i];
                let ux = if dir == SeismicDir::X {
                    floor_u[i]
                } else {
                    0.0
                };
                let uy = if dir == SeismicDir::Y {
                    floor_u[i]
                } else {
                    0.0
                };
                floor_disp.push([ux - xy[1] * theta, uy + xy[0] * theta, theta]);
            }
            let axis = usize::from(dir == SeismicDir::Y);
            let drift = vec![
                floor_disp[1][axis] - floor_disp[0][axis],
                floor_disp[2][axis] - floor_disp[1][axis],
            ];
            po.capacity_curve.push(CapacityPoint {
                step: index as u32,
                roof_disp: floor_disp[2][axis],
                base_shear: q[0] + 100.0,
                story_shear: vec![q[0] + 100.0, q[1] + 200.0],
                story_drift: drift.clone(),
            });
            po.steps.push(PushoverStep {
                load_factor: factor,
                top_disp: floor_disp[2][axis],
                base_shear: q[0] + 100.0,
                story_drifts: drift,
                floor_disp,
            });
        }
        po
    }

    #[test]
    fn nonlinear_spatial_skeleton_is_independent_of_rigid_floor_master_with_twist() {
        let (mut model, ex) = fixture(SeismicDir::X, false, false);
        let (_, ey) = fixture(SeismicDir::Y, false, false);
        let mut originals = None;
        for master_slot in [0, 3] {
            for (level, constraint) in model.constraints.iter_mut().enumerate() {
                if let Constraint::RigidDiaphragm { master, slaves, .. } = constraint {
                    *master = NodeId((level * 4 + master_slot) as u32);
                    *slaves = (0..4)
                        .filter(|k| *k != master_slot)
                        .map(|k| NodeId((level * 4 + k) as u32))
                        .collect();
                }
            }
            let px = spatial_pushover(&model, SeismicDir::X);
            let py = spatial_pushover(&model, SeismicDir::Y);
            let mut inp = input(&model, &ex, SeismicDir::X);
            inp.dim = StickDim::Spatial;
            inp.nonlinear = true;
            inp.res_y = Some(&ey);
            inp.po_x = Some(&px);
            inp.po_y = Some(&py);
            let lm = build(inp).unwrap();
            near(lm.spatial[0].skeleton_x.k1, 10000.0);
            near(lm.spatial[0].skeleton_y.k1, 10000.0);
            near(lm.spatial[1].skeleton_x.k1, 7500.0);
            near(lm.spatial[1].skeleton_y.k1, 7500.0);
            if let Some(ref original) = originals {
                let original: &LumpedMassModel = original;
                for (left, right) in lm.spatial.iter().zip(&original.spatial) {
                    for (a, b) in [
                        (left.skeleton_x, right.skeleton_x),
                        (left.skeleton_y, right.skeleton_y),
                    ] {
                        for (x, y) in [a.k1, a.d1, a.q1, a.d2, a.q2, a.d3, a.q3]
                            .into_iter()
                            .zip([b.k1, b.d1, b.q1, b.d2, b.q2, b.d3, b.q3])
                        {
                            near(x, y);
                        }
                    }
                }
            } else {
                originals = Some(lm.clone());
            }
            assert!(sepika_solver::dynamic::lumped_mass::lumped_mass_eigen(&lm, 10).is_ok());
            let response = sepika_solver::dynamic::lumped_mass::bending_shear_time_history(
                &lm,
                &[0.0, 1.0, -1.0, 0.0],
                0.01,
                0.02,
            )
            .unwrap();
            assert_eq!(response.time.len(), 4);
        }
    }

    #[test]
    fn columns_missing_intermediate_floor_or_branching_cannot_drop_out_of_ei() {
        for inclined in [false, true] {
            let (mut model, res) = fixture(SeismicDir::X, false, false);
            model
                .elements
                .retain(|e| e.id != ElemId(0) && e.id != ElemId(4));
            let mut spanning = model.elements[0].clone();
            spanning.id = ElemId(8);
            spanning.nodes = [NodeId(0), NodeId(if inclined { 9 } else { 8 })]
                .into_iter()
                .collect();
            model.elements.push(spanning);
            let error = ei_values(&model, &res, SeismicDir::X)
                .unwrap_err()
                .to_string();
            assert!(error.contains("階") && error.contains("柱 8"), "{error}");
            assert!(
                error.contains(if inclined {
                    "傾斜柱"
                } else {
                    "追跡できません"
                }),
                "{error}"
            );
        }
        let (mut model, res) = fixture(SeismicDir::X, false, true);
        let mut branch = model.elements[0].clone();
        branch.id = ElemId(model.elements.len() as u32);
        branch.nodes = [NodeId(12), NodeId(1)].into_iter().collect();
        model.elements.push(branch);
        let error = ei_values(&model, &res, SeismicDir::X)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("階") && error.contains("中間分岐"),
            "{error}"
        );
    }

    #[test]
    fn generated_panel_zone_connectivity_is_not_a_physical_story_crossing() {
        let (mut model, res) = fixture(SeismicDir::X, false, false);
        let mut panel = model.elements[0].clone();
        panel.id = ElemId(model.elements.len() as u32);
        panel.kind = sepika_core::model::ElementKind::PanelZone;
        panel.section = None;
        panel.nodes = [NodeId(4), NodeId(0), NodeId(5)].into_iter().collect();
        model.elements.push(panel);
        assert_eq!(
            signed_shears(&model, &res, SeismicDir::X).unwrap(),
            vec![2000.0, 1000.0]
        );
        let lm = build(input(&model, &res, SeismicDir::X)).unwrap();
        near(lm.stories[0].skeleton.k1, 10000.0);
    }
    #[test]
    fn generated_bilinear_shear_backbone_preserves_reduced_points() {
        use sepika_solver::nonlinear::pushover::{CapacityPoint, PushoverStep};
        let (model, res) = fixture(SeismicDir::X, false, false);
        let mut po = super::super::tests::empty_pushover();
        let mut expected = Vec::new();
        for index in 0..=30 {
            let ds = index as f64 * 0.1;
            let q = 50.0 * ds.min(2.0) + 5.0 * (ds - 2.0).max(0.0);
            let db = bending_drifts(&[3000.0; 2], &[1.2e14; 2], &[q, q]).unwrap();
            let drift = vec![ds + db[0], ds + db[1]];
            expected.push((ds, q));
            po.capacity_curve.push(CapacityPoint {
                step: index as u32,
                roof_disp: drift.iter().sum(),
                base_shear: q,
                story_shear: vec![q, q],
                story_drift: drift.clone(),
            });
            po.steps.push(PushoverStep {
                load_factor: ds,
                top_disp: drift.iter().sum(),
                base_shear: q,
                story_drifts: drift,
                floor_disp: Vec::new(),
            });
        }
        let mut inp = input(&model, &res, SeismicDir::X);
        inp.nonlinear = true;
        inp.secant_ratio = 0.9;
        inp.po_x = Some(&po);
        let lm = build(inp).unwrap();
        let expected = fit_story_trilinear(&expected, 0.9);
        assert_eq!(expected.d1, expected.d2);
        for story in &lm.stories {
            assert!(story.skeleton.is_valid_spring());
            near(story.skeleton.k1, 50.0);
            near(story.skeleton.d1, 2.0);
            near(story.skeleton.d2, 2.0);
            near(story.skeleton.q1, 100.0);
            near(story.skeleton.q2, 100.0);
            near(story.skeleton.d3, 3.0);
            near(story.skeleton.q3, 105.0);
        }
        assert!(sepika_solver::dynamic::lumped_mass::lumped_mass_eigen(&lm, 4).is_ok());
        assert!(
            sepika_solver::dynamic::lumped_mass::bending_shear_time_history(
                &lm,
                &[0.0, 10.0, -10.0, 0.0],
                0.01,
                0.02
            )
            .is_ok()
        );
    }
    #[test]
    fn out_of_scope_elements_are_rejected_without_crossing_story_midpoint() {
        use sepika_core::model::ElementKind;
        for kind in [
            ElementKind::NodalSpring,
            ElementKind::Isolator,
            ElementKind::Damper,
            ElementKind::Wall,
            ElementKind::Shell,
        ] {
            for geometry in 0..3 {
                let (mut model, res) = fixture(SeismicDir::X, false, false);
                let mut added = model.elements[0].clone();
                added.id = ElemId(model.elements.len() as u32);
                added.kind = kind;
                added.section = None;
                let mut node = model.nodes[4].clone();
                node.id = NodeId(model.nodes.len() as u32);
                node.coord[2] = if geometry < 2 { 3000.0 } else { 2400.0 };
                if geometry == 1 {
                    node.coord[0] += 100.0;
                }
                let other = node.id;
                model.nodes.push(node);
                added.nodes = [NodeId(4), other].into_iter().collect();
                if matches!(kind, ElementKind::Wall | ElementKind::Shell) {
                    added.nodes.extend([NodeId(5), NodeId(6)]);
                }
                let id = added.id;
                model.elements.push(added);
                for dim in [StickDim::Planar, StickDim::Spatial] {
                    for nonlinear in [false, true] {
                        let mut inp = input(&model, &res, SeismicDir::X);
                        inp.dim = dim;
                        inp.nonlinear = nonlinear;
                        let error = build(inp).unwrap_err().to_string();
                        assert!(
                            error.contains("階")
                                && error.contains(&format!("部材 {}", id.0))
                                && error.contains("対象外"),
                            "{kind:?}/{geometry}/{dim:?}/{nonlinear}: {error}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn active_node_support_springs_are_rejected_but_zero_or_fixed_components_are_inactive() {
        use sepika_core::dof::Dof6Mask;
        let (mut model, res) = fixture(SeismicDir::X, false, false);
        for component in 0..6 {
            model.nodes[4].restraint = Dof6Mask::FREE;
            let mut stiffness = [0.0; 6];
            stiffness[component] = 1000.0;
            model.nodes[4].support_spring = Some(stiffness);
            let error = build(input(&model, &res, SeismicDir::X))
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("階") && error.contains("節点 4") && error.contains("支持ばね"),
                "{error}"
            );
        }
        model.nodes[4].support_spring = Some([0.0; 6]);
        model.nodes[0].support_spring = Some([1000.0; 6]);
        assert!(build(input(&model, &res, SeismicDir::X)).is_ok());
        model.nodes[0].restraint = Dof6Mask::FREE;
        let error = build(input(&model, &res, SeismicDir::X))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("節点 0") && error.contains("支持ばね"),
            "{error}"
        );
    }

    #[test]
    fn horizontal_ground_restraints_above_base_are_rejected_in_the_modelled_directions() {
        use sepika_core::dof::{Dof, Dof6Mask};
        let (mut model, res) = fixture(SeismicDir::X, false, true);
        for node in [4_usize, 12] {
            model.nodes[node].restraint.set_fixed(Dof::Ux);
            let error = build(input(&model, &res, SeismicDir::X))
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("階")
                    && error.contains(&format!("節点 {}", node))
                    && error.contains("基部以外"),
                "{error}"
            );
            model.nodes[node].restraint = Dof6Mask::FREE;
        }
        model.nodes[4].restraint.set_fixed(Dof::Uy);
        assert!(validate_scope(&input(&model, &res, SeismicDir::X)).is_ok());
        let mut spatial = input(&model, &res, SeismicDir::X);
        spatial.dim = StickDim::Spatial;
        let error = validate_scope(&spatial).unwrap_err().to_string();
        assert!(
            error.contains("節点 4") && error.contains("基部以外"),
            "{error}"
        );
        assert!(validate_scope(&input(&model, &res, SeismicDir::Y)).is_err());
    }
    #[test]
    fn support_at_virtual_floor_master_and_other_constraint_nodes_is_rejected() {
        use sepika_core::dof::{Dof, Dof6Mask};
        for connection in 0..4 {
            for spring in [false, true] {
                let (mut model, res) = fixture(SeismicDir::X, false, false);
                let mut node = model.nodes[4].clone();
                node.id = NodeId(model.nodes.len() as u32);
                node.coord = [0.0, 0.0, 3000.0];
                node.restraint = Dof6Mask::FREE;
                if spring {
                    node.support_spring = Some([1000.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
                } else {
                    node.restraint.set_fixed(Dof::Ux);
                }
                let id = node.id;
                model.nodes.push(node);
                match connection {
                    0 => {
                        model.constraints[1] = Constraint::rigid_diaphragm(
                            StoryId(1),
                            id,
                            vec![NodeId(4), NodeId(5), NodeId(6), NodeId(7)],
                        )
                    }
                    1 => model.constraints.push(Constraint::RigidLink {
                        master: id,
                        slaves: vec![NodeId(4)],
                        dofs: Dof6Mask::FIXED,
                    }),
                    2 => model.constraints.push(Constraint::Mpc {
                        master: NodeId(4),
                        terms: vec![(NodeId(4), Dof::Ux, 1.0), (id, Dof::Ux, -1.0)],
                    }),
                    3 => {
                        if let Constraint::RigidDiaphragm { slaves, .. } = &mut model.constraints[1]
                        {
                            slaves.push(id);
                        }
                    }
                    _ => unreachable!(),
                }
                let error = build(input(&model, &res, SeismicDir::X))
                    .unwrap_err()
                    .to_string();
                assert!(
                    error.contains("階")
                        && error.contains(&format!("節点 {}", id.0))
                        && error.contains(if spring {
                            "支持ばね"
                        } else {
                            "基部以外"
                        }),
                    "{connection}/{spring}: {error}"
                );
            }
        }
    }
}
