use super::*;
use sepika_core::model::{FrameSectionUse, MaterialCategory};

#[derive(Clone, Debug, serde::Serialize)]
/// 指定したS造部材の鋼材量と、断面積・材長の算定元。
pub struct SteelMemberQuantity {
    pub elem: ElemId,
    pub section: SectionId,
    pub area_mm2: f64,
    pub area_basis: sepika_core::model::PropertyBasis,
    pub length_mm: f64,
    pub volume_m3: f64,
    pub steel: SteelItem,
}

/// 指定した主架構S造柱・梁の鋼材だけを集計する。空・重複・不正な対象は拒否する。
/// 他部材の型枠・コンクリート・鉄筋の算定状態を表さない。
pub fn try_compute_steel_members(
    model: &Model,
    members: &[ElemId],
) -> Result<Vec<SteelMemberQuantity>, String> {
    if members.is_empty() {
        return Err("固定対象なし".into());
    }
    let mut seen = HashSet::new();
    members
        .iter()
        .map(|id| {
            if !seen.insert(*id) {
                return Err(format!("部材 {id:?}: 固定対象が重複しています"));
            }
            let element = model
                .element(*id)
                .ok_or_else(|| format!("部材 {id:?}: 対象がありません"))?;
            if element.kind != ElementKind::Beam || element.nodes.len() != 2 {
                return Err(format!(
                    "部材 {id:?}: 2節点の主架構柱・梁を指定してください"
                ));
            }
            let section = model
                .element_section(element)
                .ok_or_else(|| format!("部材 {id:?}: 断面が未設定です"))?;
            let material = model
                .element_material(element)
                .ok_or_else(|| format!("部材 {id:?}: 材料が未設定です"))?;
            if !matches!(
                section.frame_use,
                Some(FrameSectionUse::Column | FrameSectionUse::Girder)
            ) || material.category != MaterialCategory::Steel
                || !material.young.is_finite()
                || material.young <= 0.0
                || sepika_core::structure_kind::structure_kind_of(
                    Some(section),
                    Some(material.category),
                ) != StructureKind::S
            {
                return Err(format!(
                    "部材 {id:?}: 有効な主架構S造の断面・材料・用途が必要です"
                ));
            }
            let mut coords = Vec::new();
            for node in &element.nodes {
                let coord = model
                    .node(*node)
                    .ok_or_else(|| format!("部材 {id:?}: 節点がありません"))?
                    .coord;
                if coord.iter().any(|value| !value.is_finite()) {
                    return Err(format!("部材 {id:?}: 節点座標は有限値が必要です"));
                }
                coords.push(coord);
            }
            let length_mm = dist3(coords[0], coords[1]);
            let (area_mm2, area_basis) = quantity_area_with_basis(section)?;
            let steel = steel_item(section, length_mm)?;
            if !area_mm2.is_finite()
                || area_mm2 <= 0.0
                || !length_mm.is_finite()
                || length_mm <= 0.0
                || !steel.weight_t.is_finite()
                || steel.weight_t <= 0.0
            {
                return Err(format!(
                    "部材 {id:?}: 鋼材面積・材長・重量は有限の正値が必要です"
                ));
            }
            let volume_m3 = steel.weight_t / STEEL_UNIT_WEIGHT_T_PER_M3;
            Ok(SteelMemberQuantity {
                elem: *id,
                section: section.id,
                area_mm2,
                area_basis,
                length_mm,
                volume_m3,
                steel,
            })
        })
        .collect()
}
