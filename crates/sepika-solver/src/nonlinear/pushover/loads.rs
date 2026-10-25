use crate::common::linear_load::{legacy_member_force, PhysicalLoadLedger};
use sepika_core::dof::{DofMap, DOF_PER_NODE};
use sepika_core::ids::LoadCaseId;
use sepika_core::model::{LoadCase, LoadCaseKind, MemberLoad, MemberLoadKind, Model};
use sepika_element::behavior::ElementBehavior;

pub(super) struct PhysicalPushoverLoads {
    pub(super) gravity_cases: Vec<(LoadCase, f64)>,
    pub(super) reference: LoadCase,
}

impl PhysicalPushoverLoads {
    pub(super) fn new(
        model: &Model,
        terms: &[(LoadCaseId, f64)],
        reference: LoadCase,
    ) -> Result<Self, String> {
        let gravity_cases = terms
            .iter()
            .map(|&(id, factor)| {
                let source = model
                    .load_cases
                    .iter()
                    .find(|case| case.id == id)
                    .ok_or_else(|| format!("増分解析: 重力荷重ケース {} がありません", id.0))?;
                Ok((source.clone(), factor))
            })
            .collect::<Result<_, String>>()?;
        Ok(Self {
            gravity_cases,
            reference,
        })
    }

    pub(super) fn gravity_case(&self) -> LoadCase {
        let mut combined = LoadCase {
            id: LoadCaseId(0),
            name: "増分解析の重力荷重".into(),
            nodal: Vec::new(),
            member: Vec::new(),
            kind: LoadCaseKind::Other,
        };
        for (source, factor) in &self.gravity_cases {
            combined
                .nodal
                .extend(source.nodal.iter().cloned().map(|mut load| {
                    for value in &mut load.values {
                        *value *= factor;
                    }
                    load
                }));
            combined.member.extend(
                source
                    .member
                    .iter()
                    .map(|load| scaled_member(load, *factor)),
            );
        }
        combined
    }
}

fn scaled_member(load: &MemberLoad, factor: f64) -> MemberLoad {
    let mut scaled = load.clone();
    match &mut scaled.kind {
        MemberLoadKind::Point { p, .. } => *p *= factor,
        MemberLoadKind::Distributed { w1, w2, .. } => {
            *w1 *= factor;
            *w2 *= factor;
        }
    }
    scaled
}

pub(super) fn scaled_member_table(loads: &[Vec<MemberLoad>], factor: f64) -> Vec<Vec<MemberLoad>> {
    loads
        .iter()
        .map(|row| row.iter().map(|load| scaled_member(load, factor)).collect())
        .collect()
}

pub(super) fn prepare_gravity(
    model: &Model,
    dofmap: &DofMap,
    behaviors: &[Box<dyn ElementBehavior>],
    source: &LoadCase,
) -> Result<(Vec<f64>, Vec<Vec<MemberLoad>>), String> {
    let ledger =
        PhysicalLoadLedger::new(model, source).map_err(|e| format!("増分解析・重力荷重: {e}"))?;
    if behaviors.len() != model.elements.len() {
        return Err("増分解析・重力荷重: 要素と応答要素の数が一致しません".into());
    }
    let mut rhs = vec![0.0; dofmap.n_active()];
    for (ni, nodal) in ledger.nodal.iter().enumerate() {
        for (d, value) in nodal.iter().enumerate() {
            if let Some(a) = dofmap.active(ni * DOF_PER_NODE + d) {
                rhs[a as usize] += value;
            }
        }
    }
    let mut absolute = Vec::with_capacity(behaviors.len());
    for (elem, behavior) in model.elements.iter().zip(behaviors) {
        let loads = ledger.member_loads(elem.id);
        if !behavior.owns_frame_member_loads() && !loads.is_empty() {
            let force = legacy_member_force(model, elem, loads);
            let force =
                sepika_element::frame::panel_offset::transform_nodal_load(elem, model, &force);
            let gdofs = behavior.global_dofs(dofmap);
            if force.data.len() != gdofs.len() {
                return Err(format!(
                    "要素 ID {}・増分重力荷重の自由度数が一致しません",
                    elem.id.0
                ));
            }
            for (&active, value) in gdofs.iter().zip(force.data) {
                if active != usize::MAX {
                    rhs[active] += value;
                }
            }
        }
        absolute.push(loads.to_vec());
    }
    Ok((rhs, absolute))
}
