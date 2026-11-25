use std::collections::{HashMap, HashSet};

use sepika_core::adjacency::NodeAdjacency;
use sepika_core::geom::vec3::{cross, dist, dot, norm, sub, unit};
use sepika_core::ids::{ElemId, NodeId};
use sepika_core::load_combo::{
    combination_design_state, effective_combination_terms, LoadDuration,
};
use sepika_core::model::{
    ElementData, ElementKind, FrameSectionUse, MemberLoad, Model, SupportMemberId,
    WallSideColumnIndex,
};
use sepika_core::section_shape::SectionShape;
use sepika_element::frame::beam::MemberForces;
use sepika_element::frame::member_load::{frame_shear_envelope, FrameShearEnvelope};
use sepika_element::transform::LocalFrame;

use super::RcColumnLongTermShearInput;
use crate::design_position::is_near_design_position;

pub(crate) type LongTermColumnShear = Result<[Result<FrameShearEnvelope, String>; 2], String>;

pub(super) enum ColumnShearLoads<'a> {
    SingleCase,
    Unavailable(String),
    Combination {
        by_member: HashMap<ElemId, Vec<(f64, &'a MemberLoad)>>,
        wall_columns: HashSet<ElemId>,
        panel_nodes: HashSet<NodeId>,
        frame_index: NodeAdjacency,
    },
}

pub(super) fn missing_design_positions(required: &[f64], forces: &MemberForces) -> Vec<f64> {
    let mut observed = vec![false; required.len()];
    for &(pos, _) in &forces.at {
        if let Some((index, _)) = required
            .iter()
            .enumerate()
            .filter(|(_, target)| is_near_design_position(pos, std::slice::from_ref(*target)))
            .min_by(|(_, a), (_, b)| (pos - *a).abs().total_cmp(&(pos - *b).abs()))
        {
            observed[index] = true;
        }
    }
    required
        .iter()
        .copied()
        .zip(observed)
        .filter_map(|(pos, has_response)| (!has_response).then_some(pos))
        .collect()
}

impl<'a> ColumnShearLoads<'a> {
    pub(super) fn from_input(model: &'a Model, input: RcColumnLongTermShearInput<'_>) -> Self {
        match input {
            RcColumnLongTermShearInput::SingleCase => Self::SingleCase,
            RcColumnLongTermShearInput::Unverified => {
                Self::Unavailable("解析で用いた保存荷重組合せと生成入力が未確認です".into())
            }
            RcColumnLongTermShearInput::SavedCombination(name) => {
                Self::from_combination(model, name).unwrap_or_else(Self::Unavailable)
            }
        }
    }

    fn from_combination(model: &'a Model, name: &str) -> Result<Self, String> {
        if sepika_core::wall_expand::model_has_wall_plates_to_expand(model)
            && model.generated_wall_origins.is_empty()
            && sepika_core::wall_expand::expand_wall_elements(model)
                .2
                .generated
                > 0
        {
            return Err("解析時の壁要素を含む展開済みモデルがありません".into());
        }
        let mut combos = model.combinations.iter().filter(|combo| combo.name == name);
        let combo = combos.next().ok_or("保存荷重組合せがありません")?;
        if combos.next().is_some() {
            return Err("保存荷重組合せ名が重複しています".into());
        }
        if combination_design_state(combo, &model.load_cases)?.duration != LoadDuration::Long {
            return Err("保存荷重組合せが長期荷重用ではありません".into());
        }
        if model.stress_cfg.tension_only_iteration
            && model
                .elements
                .iter()
                .any(|elem| matches!(elem.kind, ElementKind::Brace { tension_only: true }))
        {
            return Err("引張専用ブレースの別ケース合成は同一解を保証できません".into());
        }
        let mut by_member: HashMap<_, Vec<_>> = HashMap::new();
        for (id, factor) in effective_combination_terms(combo, &model.load_cases)? {
            if factor == 0.0 {
                continue;
            }
            let mut cases = model.load_cases.iter().filter(|case| case.id == id);
            let case = cases
                .next()
                .ok_or("保存組合せが参照する荷重ケースがありません")?;
            if cases.next().is_some() {
                return Err("保存組合せが参照する荷重ケースIDが重複しています".into());
            }
            for load in &case.member {
                by_member.entry(load.elem).or_default().push((factor, load));
            }
        }
        let wall_columns = WallSideColumnIndex::build(model)
            .bindings()
            .iter()
            .map(|binding| binding.column)
            .collect();
        let panel_nodes = model
            .elements
            .iter()
            .filter(|e| e.kind == ElementKind::PanelZone)
            .filter_map(|e| e.nodes.first().copied())
            .collect();
        Ok(Self::Combination {
            by_member,
            wall_columns,
            panel_nodes,
            frame_index: NodeAdjacency::primary_frame(model),
        })
    }

    pub(super) fn requires_envelope(&self) -> bool {
        !matches!(self, Self::SingleCase)
    }

    pub(super) fn envelope(
        &self,
        model: &Model,
        elem: &ElementData,
        forces: &MemberForces,
    ) -> Option<LongTermColumnShear> {
        Some(match self {
            Self::SingleCase => return None,
            Self::Unavailable(reason) => Err(reason.clone()),
            Self::Combination {
                by_member,
                wall_columns,
                panel_nodes,
                frame_index,
            } => {
                let loads = by_member.get(&elem.id).map_or(&[][..], Vec::as_slice);
                if wall_columns.contains(&elem.id) {
                    Err("壁側柱の柱区間と壁境界が未確認です".into())
                } else if elem.end_offsets_mm != [[0.0; 3]; 2] {
                    Err("XYZ偏心がある柱の実材軸上の材端フェースが未確認です".into())
                } else if elem.nodes.iter().any(|id| panel_nodes.contains(id))
                    || elem.rigid_zone.panel_offset_i != 0.0
                    || elem.rigid_zone.panel_offset_j != 0.0
                {
                    Err("仕口パネルに接続する柱の材端フェースが未確認です".into())
                } else {
                    column_envelope(model, frame_index, elem, loads, forces)
                }
            }
        })
    }
}

fn column_envelope(
    model: &Model,
    frame_index: &NodeAdjacency,
    elem: &ElementData,
    loads: &[(f64, &MemberLoad)],
    forces: &MemberForces,
) -> LongTermColumnShear {
    let &[i_node, j_node] = elem.nodes.as_slice() else {
        return Err("柱の両材端節点がありません".into());
    };
    let (i, j) = model
        .node(i_node)
        .zip(model.node(j_node))
        .ok_or("柱の両材端節点がありません")?;
    let frame = LocalFrame::from_nodes(i.coord, j.coord, elem.local_axis.ref_vector)?;
    let length_mm = dist(i.coord, j.coord);
    let (Some(prepared_i_mm), Some(prepared_j_mm)) =
        (elem.rigid_zone.face_i, elem.rigid_zone.face_j)
    else {
        return Err("柱の両材端フェース位置が未算定です".into());
    };
    let face_i_mm = column_face_distance(
        model,
        frame_index,
        elem.id,
        i_node,
        i.coord,
        frame.rot[0],
        prepared_i_mm,
    )
    .map_err(|reason| format!("i端の材端フェースが未確定です（{reason}）"))?;
    let face_j_mm = column_face_distance(
        model,
        frame_index,
        elem.id,
        j_node,
        j.coord,
        frame.rot[0].map(|v| -v),
        prepared_j_mm,
    )
    .map_err(|reason| format!("j端の材端フェースが未確定です（{reason}）"))?;
    frame_shear_envelope(
        &frame,
        length_mm,
        [face_i_mm, length_mm - face_j_mm],
        loads,
        forces,
    )
}

fn column_face_distance(
    model: &Model,
    frame_index: &NodeAdjacency,
    column: ElemId,
    node: NodeId,
    origin_mm: [f64; 3],
    inward: [f64; 3],
    prepared_mm: f64,
) -> Result<f64, String> {
    if !prepared_mm.is_finite() || prepared_mm < 0.0 {
        return Err("解析準備で得たフェース距離が不正です".into());
    }
    let parallel = |axis| norm(cross(axis, inward)) <= 32.0 * f64::EPSILON;
    let mut distance_mm = None;
    for other in frame_index
        .elements_at(model, node)
        .filter(|other| other.id != column)
    {
        let section = model
            .element_section(other)
            .ok_or("接続部材の断面がありません")?;
        match section.frame_use {
            Some(FrameSectionUse::Column) => {
                let ends = model.primary_frame_axis_ends(other)?;
                let other_node = other
                    .nodes
                    .iter()
                    .find(|&&id| id != node)
                    .and_then(|id| model.node(*id))
                    .ok_or("接続柱の材軸がありません")?;
                if ends.is_some()
                    || unit(sub(other_node.coord, origin_mm))
                        .is_none_or(|axis| !parallel(axis) || dot(axis, inward) >= 0.0)
                {
                    return Err("柱の材軸が連続しない接合です".into());
                }
            }
            Some(FrameSectionUse::Girder) => {
                let placement =
                    model.calculation_beam_placement(SupportMemberId::Primary(other.id))?;
                if !matches!(
                    placement.section_shape,
                    SectionShape::RcBeamRect { .. } | SectionShape::SrcBeamRect { .. }
                ) || !placement.axes[1..].iter().copied().any(parallel)
                {
                    return Err(format!(
                        "大梁 ID {} と柱の接合境界を一つの断面位置で表せません",
                        other.id.0
                    ));
                }
                let face_mm = placement
                    .connection_face_at_contact_mm(origin_mm, inward)?
                    .filter(|face| face.is_finite())
                    .ok_or_else(|| {
                        format!(
                            "大梁 ID {} の実配置と柱の接合境界を確認できません",
                            other.id.0
                        )
                    })?;
                if distance_mm.is_some_and(|previous| previous != face_mm) {
                    return Err("接続大梁によって材端フェースの位置が異なります".into());
                }
                distance_mm = Some(face_mm);
            }
            _ => return Err(format!("接続部材 ID {} の断面用途が未確認です", other.id.0)),
        }
    }
    if distance_mm.is_none() && prepared_mm > 0.0 {
        return Err("解析準備で得たフェースに対応する接続大梁を確認できません".into());
    }
    Ok(distance_mm.unwrap_or(0.0))
}
