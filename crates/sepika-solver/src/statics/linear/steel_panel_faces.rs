use super::StaticOnce;
use sepika_core::adjacency::NodeAdjacency;
use sepika_core::ids::ElemId;
use sepika_core::model::Model;
use sepika_core::panel_zone::{
    member_orientation, panel_joint_planes, select_panel_column, steel_panel_column_sections,
    MemberOrientation, PanelColumnSection,
};
use sepika_core::structure_kind::{member_structure_kind, StructureKind};
use sepika_element::frame::member_load::LoadedFrameSection;
use sepika_math::solver::SolveError;

/// 仕口パネルのフランジ面位置における、引張正の柱軸力 [N]。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SteelPanelColumnFaceForce {
    pub element: ElemId,
    pub position_mm: f64,
    pub axial_n: Result<f64, String>,
}

pub(super) fn recovery_sections(
    model: &Model,
    adjacency: &NodeAdjacency,
) -> Vec<Vec<PanelColumnSection>> {
    let mut sections = vec![Vec::<PanelColumnSection>::new(); model.elements.len()];
    for node in &model.nodes {
        let members = || adjacency.elements_at(model, node.id);
        let columns: Vec<_> = members()
            .filter(|element| member_orientation(model, element) == Some(MemberOrientation::Column))
            .collect();
        if columns.len() != 2
            || members()
                .filter(|element| member_orientation(model, element).is_some())
                .any(|element| member_structure_kind(model, element) != StructureKind::S)
        {
            continue;
        }
        let Ok(adopted) = select_panel_column(model, node.id, &columns) else {
            continue;
        };
        let planes = panel_joint_planes(model, node.id, members());
        for flange in planes
            .iter()
            .flat_map(|plane| &plane.beams)
            .filter_map(|beam| beam.flanges.as_ref().ok())
        {
            let Ok(cuts) =
                steel_panel_column_sections(model, node.id, adopted.id, *flange, members())
            else {
                continue;
            };
            for cut in cuts {
                let positions = &mut sections[cut.element_position];
                if !positions.iter().any(|p| p.position_mm == cut.position_mm) {
                    positions.push(cut);
                }
            }
        }
    }
    sections
}

pub(super) fn recover(
    cuts: &[PanelColumnSection],
    section: Option<&LoadedFrameSection>,
    result: &mut Vec<SteelPanelColumnFaceForce>,
) {
    result.extend(cuts.iter().map(|cut| {
        let axial_n = section
            .ok_or_else(|| "荷重込み断面力を回収できません".into())
            .and_then(|section| section.at_continuous_section_mm(cut.position_mm, cut.length_mm))
            .map(|force| force[0])
            .map_err(|reason| {
                format!(
                    "柱 ID {} の仕口フランジ位置 {} mm: {reason}",
                    cut.element.0, cut.position_mm
                )
            });
        SteelPanelColumnFaceForce {
            element: cut.element,
            position_mm: cut.position_mm,
            axial_n,
        }
    }));
}

pub(super) fn superpose(
    terms: &[(&StaticOnce, f64)],
) -> Result<Vec<SteelPanelColumnFaceForce>, SolveError> {
    let nonzero = || terms.iter().filter(|(_, factor)| *factor != 0.0);
    let base = nonzero().next().or_else(|| {
        terms
            .iter()
            .find(|(res, _)| !res.steel_panel_column_face_forces.is_empty())
    });
    let mut result = base.map_or_else(Vec::new, |(res, _)| {
        res.steel_panel_column_face_forces.clone()
    });
    for cut in &mut result {
        if let Ok(value) = &mut cut.axial_n {
            *value = 0.0;
        }
    }
    for (response, factor) in nonzero() {
        if result.len() != response.steel_panel_column_face_forces.len() {
            return Err(SolveError::InvalidInput(
                "合成する結果間で仕口フランジ面の断面数が一致しません".into(),
            ));
        }
        for (cut, source) in result
            .iter_mut()
            .zip(&response.steel_panel_column_face_forces)
        {
            if cut.element != source.element || cut.position_mm != source.position_mm {
                return Err(SolveError::InvalidInput(
                    "合成する結果間で仕口フランジ面の柱または評価位置が一致しません".into(),
                ));
            }
            if let Ok(value) = cut.axial_n {
                cut.axial_n = source
                    .axial_n
                    .as_ref()
                    .map_err(Clone::clone)
                    .and_then(|current| {
                        let next = value + factor * current;
                        if next.is_finite() {
                            Ok(next)
                        } else {
                            Err(format!(
                                "柱 ID {} の合成フランジ面軸力が非有限です",
                                cut.element.0
                            ))
                        }
                    });
            }
        }
    }
    Ok(result)
}
