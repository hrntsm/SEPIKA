use super::assemble::{member_load_frame, span_load_transfer};
use sepika_core::dof::{DofMap, DOF_PER_NODE};
use sepika_core::ids::ElemId;
use sepika_core::model::{ElementData, LoadCase, MemberLoad, Model};
use sepika_element::behavior::{ElementBehavior, LoadedFrameResponse};
use sepika_math::solver::SolveError;
use std::collections::HashMap;

pub(crate) struct PhysicalLoadLedger<'a> {
    pub(crate) source: &'a LoadCase,
    pub(crate) nodal: Vec<[f64; DOF_PER_NODE]>,
    member_by_elem: HashMap<ElemId, Vec<MemberLoad>>,
}

impl<'a> PhysicalLoadLedger<'a> {
    pub(crate) fn new(model: &Model, source: &'a LoadCase) -> Result<Self, SolveError> {
        let mut nodal = vec![[0.0; DOF_PER_NODE]; model.nodes.len()];
        for load in &source.nodal {
            let values = nodal.get_mut(load.node.index()).ok_or_else(|| {
                SolveError::InvalidInput(format!(
                    "線形解析: 荷重節点 {} が存在しません",
                    load.node.0
                ))
            })?;
            for (value, load_value) in values.iter_mut().zip(load.values) {
                *value += load_value;
            }
        }
        let mut member_by_elem: HashMap<ElemId, Vec<MemberLoad>> = HashMap::new();
        for load in &source.member {
            if !model.elements.iter().any(|elem| elem.id == load.elem) {
                return Err(element_error(load.elem, "荷重対象の要素が存在しません"));
            }
            member_by_elem
                .entry(load.elem)
                .or_default()
                .push(load.clone());
        }
        Ok(Self {
            source,
            nodal,
            member_by_elem,
        })
    }

    pub(crate) fn member_loads(&self, id: ElemId) -> &[MemberLoad] {
        self.member_by_elem
            .get(&id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

pub(crate) fn element_error(id: ElemId, reason: impl std::fmt::Display) -> SolveError {
    SolveError::InvalidInput(format!("要素 ID {}・線形解析: {reason}", id.0))
}

pub(crate) fn loaded_response(
    elem: &ElementData,
    behavior: &dyn ElementBehavior,
    u_elem: &[f64],
    loads: &[MemberLoad],
) -> Result<LoadedFrameResponse, SolveError> {
    let response = behavior
        .frame_load_response(u_elem, loads)
        .map_err(|reason| element_error(elem.id, reason))?
        .ok_or_else(|| element_error(elem.id, "部材荷重所有要素の応答がありません"))?;
    if elem.nodes.len() != 2 || response.nodal_residual.data.len() != behavior.n_dof() {
        return Err(element_error(
            elem.id,
            "部材荷重応答の自由度数が一致しません",
        ));
    }
    Ok(response)
}

pub(crate) fn legacy_member_force(
    model: &Model,
    elem: &ElementData,
    loads: &[MemberLoad],
) -> [f64; 12] {
    let Some((frame, length)) = member_load_frame(model, elem) else {
        return [0.0; 12];
    };
    let local = sepika_element::frame::member_load::consistent_load_local(
        loads,
        &frame,
        length,
        span_load_transfer(elem),
    );
    frame.rotate_to_global(&local)
}

pub(crate) fn validate_entries(
    model: &Model,
    dofmap: &DofMap,
    entries: &[(ElemId, &dyn ElementBehavior, &[usize])],
) -> Result<(), SolveError> {
    if entries.len() != model.elements.len() {
        return Err(SolveError::InvalidInput(
            "線形解析: 要素と応答要素の数が一致しません".into(),
        ));
    }
    let full = dofmap.including_restrained_node_dofs();
    for (elem, &(id, behavior, gdofs)) in model.elements.iter().zip(entries) {
        if elem.id != id
            || gdofs != behavior.global_dofs(dofmap).as_slice()
            || gdofs.len() != behavior.n_dof()
        {
            return Err(element_error(
                elem.id,
                "応答要素の順序または自由度対応が一致しません",
            ));
        }
        let mut actual_nodes: Vec<_> = behavior
            .global_dofs(&full)
            .iter()
            .filter(|&&a| a != usize::MAX)
            .map(|&a| full.global(a as u32))
            .filter(|&g| full.is_node_dof(g))
            .map(|g| g / DOF_PER_NODE)
            .collect();
        actual_nodes.sort_unstable();
        actual_nodes.dedup();
        let mut expected_nodes: Vec<_> = elem.nodes.iter().map(|node| node.index()).collect();
        expected_nodes.sort_unstable();
        expected_nodes.dedup();
        if behavior.owns_frame_member_loads() && actual_nodes != expected_nodes {
            return Err(element_error(elem.id, "応答要素の物理節点が一致しません"));
        }
    }
    Ok(())
}

pub(crate) fn assemble_linear_rhs(
    model: &Model,
    dofmap: &DofMap,
    physical: &PhysicalLoadLedger<'_>,
    entries: &[(ElemId, &dyn ElementBehavior, &[usize])],
) -> Result<Vec<f64>, SolveError> {
    validate_entries(model, dofmap, entries)?;
    let mut rhs = vec![0.0; dofmap.n_active()];
    for (ni, nodal) in physical.nodal.iter().enumerate() {
        for (d, value) in nodal.iter().enumerate() {
            if let Some(a) = dofmap.active(ni * DOF_PER_NODE + d) {
                rhs[a as usize] += value;
            }
        }
    }
    for (elem, &(_, behavior, gdofs)) in model.elements.iter().zip(entries) {
        let loads = physical.member_loads(elem.id);
        if behavior.owns_frame_member_loads() {
            let response = loaded_response(elem, behavior, &vec![0.0; gdofs.len()], loads)?;
            for (&a, force) in gdofs.iter().zip(response.nodal_residual.data) {
                if a != usize::MAX {
                    rhs[a] -= force;
                }
            }
        } else if !loads.is_empty() {
            let force = legacy_member_force(model, elem, loads);
            for (i, node) in elem.nodes.iter().take(2).enumerate() {
                for d in 0..DOF_PER_NODE {
                    if let Some(a) = dofmap.active(node.index() * DOF_PER_NODE + d) {
                        rhs[a as usize] += force[i * DOF_PER_NODE + d];
                    }
                }
            }
        }
    }
    Ok(rhs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sepika_element::behavior::{Ctx, LocalMat, LocalVec, MassOption};

    struct BrokenOwner(bool);

    impl ElementBehavior for BrokenOwner {
        fn n_dof(&self) -> usize {
            12
        }
        fn global_dofs(&self, _: &DofMap) -> smallvec::SmallVec<[usize; 24]> {
            smallvec::smallvec![usize::MAX; 12]
        }
        fn tangent_stiffness(&self, _: &Ctx) -> LocalMat {
            LocalMat::zeros(12)
        }
        fn internal_force(&self, _: &Ctx) -> LocalVec {
            LocalVec {
                data: smallvec::smallvec![0.0; 12],
            }
        }
        fn mass_matrix(&self, _: MassOption) -> LocalMat {
            LocalMat::zeros(12)
        }
        fn owns_frame_member_loads(&self) -> bool {
            true
        }
        fn frame_load_response(
            &self,
            _: &[f64],
            _: &[MemberLoad],
        ) -> Result<Option<LoadedFrameResponse>, String> {
            if self.0 {
                Err("独立試験の局所失敗".into())
            } else {
                Ok(None)
            }
        }
    }

    #[test]
    fn owner_none_and_error_keep_element_and_analysis_diagnostics() {
        let elem = ElementData {
            id: ElemId(73),
            kind: sepika_core::model::ElementKind::Beam,
            nodes: smallvec::smallvec![sepika_core::ids::NodeId(0), sepika_core::ids::NodeId(1)],
            section: None,
            local_axis: sepika_core::model::LocalAxis {
                ref_vector: [0.0, 0.0, 1.0],
            },
            end_cond: [sepika_core::model::EndCondition::Fixed; 2],
            force_regime: sepika_core::model::ForceRegime::Auto,
            rigid_zone: Default::default(),
            plastic_zone: None,
            spring: None,
        };
        for fail in [false, true] {
            let error = loaded_response(&elem, &BrokenOwner(fail), &[0.0; 12], &[])
                .err()
                .unwrap()
                .to_string();
            assert!(error.contains("73") && error.contains("線形解析"));
            assert!(error.contains(if fail {
                "独立試験の局所失敗"
            } else {
                "応答がありません"
            }));
        }
    }
}
