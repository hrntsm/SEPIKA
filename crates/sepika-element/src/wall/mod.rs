//! 壁要素。
//!
//! - [`wall_element`] —  耐震壁（壁エレメントモデル）要素
//! - [`misc_wall`] —   フレーム内雑壁の判定・幾何
//! - [`side_column`] — 耐震壁の側柱
pub mod misc_wall;
pub mod shear_section;
pub mod side_column;
pub mod static_face;
pub mod wall_element;

/// 壁と同じ主材の専用大梁断面を割り当て、上下辺へ大梁を追加する（テスト用）。
/// 側柱は追加しない。必要な試験は断面付きの線材を別途追加する。
/// 壁の幾何を取得できない場合はpanicする。
#[cfg(test)]
pub(crate) fn add_surrounding_frame(
    model: &mut sepika_core::model::Model,
    wall: &sepika_core::model::ElementData,
) {
    use sepika_core::ids::{ElemId, SectionId};
    use sepika_core::model::{
        ElementData, ElementKind, EndCondition, ForceRegime, FrameSectionUse, LocalAxis, Section,
    };

    let g = wall_element::wall_element_geometry(wall, model).expect("壁の幾何を取得できない");
    let base = model.elements.iter().map(|e| e.id.0).max().unwrap_or(0) + 1;
    let section_id = SectionId(model.sections.len() as u32);
    let mut section = Section::zero(section_id, "壁上下梁".into());
    section.frame_use = Some(FrameSectionUse::Girder);
    section.material = model.element_material(wall).map(|m| m.id);
    model.sections.push(section);
    for (i, (a, b)) in [(g.bottom[0], g.bottom[1]), (g.top[0], g.top[1])]
        .into_iter()
        .enumerate()
    {
        model.elements.push(ElementData {
            id: ElemId(base + i as u32),
            kind: ElementKind::Beam,
            nodes: smallvec::smallvec![a, b],
            section: Some(section_id),
            local_axis: LocalAxis {
                ref_vector: [0.0, 0.0, 1.0],
            },
            end_cond: [EndCondition::Fixed, EndCondition::Fixed],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            vertical_eccentricity: Some(0.0),
            plastic_zone: None,
            spring: None,
        });
    }
}
