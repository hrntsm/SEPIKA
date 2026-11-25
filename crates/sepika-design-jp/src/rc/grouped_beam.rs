//! 分割RC矩形大梁のQD1用スパン。

use super::beam::{
    beam_qd1_end_moments, beam_qd1_face_distance, validate_beam_qd1_section_geometry,
};
use crate::material_strength::{
    main_rebar_category_issue, main_rebar_grade, main_rebar_grade_requiring_approval,
};
use crate::MemberKind;
use sepika_core::adjacency::NodeAdjacency;
use sepika_core::dof::Dof6Mask;
use sepika_core::geom::vec3::{dist, local_frame_axes};
use sepika_core::ids::ElemId;
use sepika_core::model::{Constraint, ElementData, EndCondition, MaterialCategory, Model};
use sepika_core::section_shape::SectionShape;

/// 一本部材グループの、全区間で共通の局所基底と実材軸区間（並び順、長さは mm）。
pub struct GroupedBeamQd1Axis<'a> {
    pub axes: [[f64; 3]; 3],
    pub parts: Vec<(&'a ElementData, f64)>,
}

/// 注目する一本部材グループが中間支点のない連続RC矩形大梁か、局所座標系が一貫するかを確認する。
/// 区間は i → j の順で連結し、各区間を他の一本部材グループと共有してはならない。
pub fn grouped_beam_qd1_axis<'a>(
    model: &'a Model,
    group: &[ElemId],
) -> Result<GroupedBeamQd1Axis<'a>, String> {
    if group.len() < 2 {
        return Err("QD1用の一本部材の分割区間が不足しています".into());
    }
    let mut axis = None;
    let mut parts: Vec<(&ElementData, f64)> = Vec::with_capacity(group.len());
    let mut previous_end = None;
    for (index, id) in group.iter().copied().enumerate() {
        if group[..index].contains(&id)
            || model
                .girder_groups
                .iter()
                .filter(|g| g.contains(&id))
                .count()
                != 1
        {
            return Err(format!("QD1用の一本部材の区間 {} が重複しています", id.0));
        }
        let elem = model
            .element(id)
            .ok_or_else(|| format!("QD1用の一本部材の区間 {} が見つかりません", id.0))?;
        if !matches!(
            MemberKind::try_of_element(elem, model),
            Ok(MemberKind::Girder)
        ) || !model
            .element_material(elem)
            .is_some_and(|material| material.category == MaterialCategory::Concrete)
            || !matches!(
                model
                    .element_section(elem)
                    .and_then(|section| section.shape.as_ref()),
                Some(SectionShape::RcBeamRect { .. })
            )
        {
            return Err(format!(
                "QD1用の一本部材の区間 {} をRC矩形大梁と確認できません",
                id.0
            ));
        }
        validate_beam_qd1_section_geometry(model, elem)?;
        let [i, j] = model
            .primary_frame_axis_end_positions(elem)?
            .ok_or_else(|| format!("QD1用の一本部材の区間 {} の実材軸が不明です", id.0))?;
        let axes = local_frame_axes(i, j, elem.local_axis.ref_vector)
            .map_err(|reason| format!("QD1用の一本部材の区間 {} の局所軸: {reason}", id.0))?;
        let reference = axis.get_or_insert(axes);
        if reference.iter().zip(axes).any(|(a, b)| dist(*a, b) > 1e-9) {
            return Err(format!(
                "QD1用の一本部材の区間 {} で局所軸の向きが一致しません",
                id.0
            ));
        }
        let length_mm = dist(i, j);
        if !length_mm.is_finite() || length_mm <= 1e-9 {
            return Err(format!("QD1用の一本部材の区間 {} の材長が不正です", id.0));
        }
        if let Some((previous, _)) = parts.last() {
            if previous.nodes[1] != elem.nodes[0]
                || previous_end.is_none_or(|end| dist(end, i) > 1e-6)
                || previous.end_offsets_mm[1] != [0.0; 3]
                || elem.end_offsets_mm[0] != [0.0; 3]
                || [
                    previous.rigid_zone.length_j,
                    previous.rigid_zone.panel_offset_j,
                    previous.rigid_zone.face_j.unwrap_or(0.0),
                    elem.rigid_zone.length_i,
                    elem.rigid_zone.panel_offset_i,
                    elem.rigid_zone.face_i.unwrap_or(0.0),
                ]
                .into_iter()
                .any(|value| value != 0.0)
            {
                return Err(format!(
                    "QD1用の一本部材の区間 {} のi端で実材軸の連続性を確認できません",
                    id.0
                ));
            }
            if previous.end_cond[1] != EndCondition::Fixed
                || elem.end_cond[0] != EndCondition::Fixed
            {
                return Err(format!(
                    "QD1用の一本部材の区間 {} のi端で強軸曲げの連続性を確認できません",
                    id.0
                ));
            }
            let node_id = elem.nodes[0];
            let isolated = model.node(node_id).is_some_and(|node| {
                node.restraint == Dof6Mask::FREE
                    && node
                        .support_spring
                        .is_none_or(|spring| spring.iter().all(|&k| k == 0.0))
            }) && !model.elements.iter().any(|other| {
                other.id != elem.id && other.id != previous.id && other.nodes.contains(&node_id)
            }) && !model.constraints.iter().any(|constraint| match constraint {
                Constraint::RigidDiaphragm { master, slaves, .. }
                | Constraint::RigidLink { master, slaves, .. } => {
                    *master == node_id || slaves.contains(&node_id)
                }
                Constraint::Mpc { master, terms } => {
                    *master == node_id || terms.iter().any(|(id, _, _)| *id == node_id)
                }
            });
            if !isolated {
                return Err(format!(
                    "QD1用の一本部材の中間節点 {} に他部材または支持・拘束があります",
                    node_id.0
                ));
            }
        }
        parts.push((elem, length_mm));
        previous_end = Some(j);
    }
    let axes = axis.expect("2区間以上を確認済み");
    Ok(GroupedBeamQd1Axis { axes, parts })
}

pub(crate) fn grouped_beam_qd1_mechanism(
    model: &Model,
    adjacency: &NodeAdjacency,
    group: &[ElemId],
) -> Result<(f64, f64), String> {
    let span = grouped_beam_qd1_axis(model, group)?;
    let end_i = span.parts[0];
    let end_j = span.parts[span.parts.len() - 1];
    let mut clear_mm = span.parts.iter().map(|(_, l)| l).sum::<f64>();
    let mut moments_nmm = [[0.0; 2]; 2];
    for (side, (elem, length_mm)) in [end_i, end_j].into_iter().enumerate() {
        let face_mm = beam_qd1_face_distance(
            model,
            adjacency,
            elem,
            side,
            span.axes[2],
            Some(span.axes[1]),
        )?;
        if face_mm >= length_mm {
            return Err("QD1用の一本部材の外端フェースが端の分割区間に収まりません".into());
        }
        clear_mm -= face_mm;
        let shape = model
            .element_section(elem)
            .and_then(|section| section.shape.as_ref())
            .ok_or("QD1用の一本部材の外端断面を確認できません")?;
        let rebar_material = model.element_rebar_material(elem);
        let rebar_valid = main_rebar_category_issue(rebar_material).is_none()
            && main_rebar_grade_requiring_approval(main_rebar_grade(rebar_material)).is_none();
        moments_nmm[side] = beam_qd1_end_moments(shape, rebar_material, rebar_valid)
            .map_err(|reason| format!("一本部材の外端（区間 {}）: {reason}", elem.id.0))?;
    }
    if !clear_mm.is_finite() || clear_mm <= 0.0 {
        return Err("QD1用の一本部材に有限で正の内法長さが成立しません".into());
    }
    let [[top_i, bottom_i], [top_j, bottom_j]] = moments_nmm;
    Ok((clear_mm, (top_i + bottom_j).max(bottom_i + top_j)))
}
