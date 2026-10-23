//! 均等配置の必要横補剛箇所数（参考情報）の適用確認。

use sepika_core::ids::{ElemId, NodeId, SectionId};
use sepika_core::model::{
    ElementData, ElementKind, EndCondition, FrameSectionUse, Material, MaterialCategory, Model,
    Section,
};
use sepika_core::section_shape::SectionShape;

/// 柱間節点全長に対する参考数。実配置・補剛性能の適合を表さない。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum LateralBracingReference {
    Available {
        required_count: u32,
        length_mm: f64,
        governing_element: ElemId,
        section: SectionId,
        section_name: String,
        material_name: String,
        lambda_y: f64,
    },
    Unavailable {
        reason: String,
    },
}

pub(crate) const UNCONFIRMED_DETAIL: &str =
    "必要横補剛箇所数（参考）: 算定不能（柱間全長・端条件未確認）。実配置・補剛性能は未検定";

impl LateralBracingReference {
    pub(crate) fn detail(&self) -> String {
        match self {
            Self::Available { required_count, length_mm, governing_element, section_name, material_name, lambda_y, .. } => format!(
                "必要横補剛箇所数（参考）n={required_count} (柱間節点全長L={length_mm:.3} mm, 支配要素={}, 断面={section_name}, 材種={material_name}, λy={lambda_y:.3})。モデルに表現された区間断面のみ評価。未表現の断面変化・実配置・補剛性能は未検定", governing_element.0),
            Self::Unavailable { reason } => format!("必要横補剛箇所数（参考）: 算定不能（{reason}）。実配置・補剛性能は未検定"),
        }
    }
}

fn positive(value: f64, name: &str) -> Result<(), String> {
    if !value.is_finite() || value <= 0.0 {
        Err(format!("{name}が非有限または正値でありません"))
    } else {
        Ok(())
    }
}

fn class_coefficient(name: &str) -> Result<f64, String> {
    match name.trim().to_ascii_uppercase().as_str() {
        "SS400" | "SN400" | "SN400A" | "SN400B" | "SN400C" | "SM400" | "SM400A" | "SM400B"
        | "SM400C" => Ok(170.0),
        "SS490" | "SN490" | "SN490B" | "SN490C" | "SM490" | "SM490A" | "SM490B" | "SM490C"
        | "SM490YA" | "SM490YB" => Ok(130.0),
        _ => Err(format!("材種{name}は400/490級の明示対象外です")),
    }
}

fn count(length: f64, sec: &Section, mat: &Material) -> Result<(u32, f64), String> {
    if let Some(reason) =
        sepika_core::material_grade::steel_f_value_issue(&mat.name, super::plate_thickness(sec))
    {
        return Err(reason);
    }
    let coefficient = class_coefficient(&mat.name)?;
    if mat.category != MaterialCategory::Steel || sec.frame_use != Some(FrameSectionUse::Girder) {
        return Err("明示した鋼材・大梁用途が必要です".into());
    }
    let Some(SectionShape::SteelH {
        height,
        width,
        web_thick,
        flange_thick,
        root_r,
    }) = sec.shape.as_ref()
    else {
        return Err("明示SteelH断面以外は対象外です".into());
    };
    for (v, name) in [
        (*height, "H高さ"),
        (*width, "H幅"),
        (*web_thick, "Hウェブ厚"),
        (*flange_thick, "Hフランジ厚"),
        (sec.area, "A"),
        (sec.iz, "Iz"),
        (sec.iy, "Iy"),
        (length, "L"),
        (mat.young, "E"),
    ] {
        positive(v, name)?;
    }
    if 2.0 * flange_thick >= *height || web_thick >= width || sec.iz > sec.iy {
        return Err("H寸法または弱軸Iz規約が不整合です".into());
    }
    if root_r.is_some_and(|r| {
        !r.is_finite()
            || r < 0.0
            || 2.0 * r > height - 2.0 * flange_thick
            || 2.0 * r > width - web_thick
    }) {
        return Err("Hフィレット寸法が不正です".into());
    }
    if let Some(fy) = mat.fy {
        positive(fy, "入力fy")?;
    }
    if let Some(factor) = mat.strength_factor {
        positive(factor, "強度割増係数")?;
    }
    let f = super::steel_f_value_prefix(
        &mat.name.trim().to_ascii_uppercase(),
        super::plate_thickness(sec),
    )
    .or(mat.fy)
    .ok_or("基準強度が未確認です")?;
    positive(f, "F")?;
    let radius_sq = sec.iz / sec.area;
    positive(radius_sq, "Iz/A")?;
    let radius = radius_sq.sqrt();
    positive(radius, "弱軸二次半径")?;
    let lambda = length / radius;
    positive(lambda, "λy")?;
    let n = ((lambda - coefficient) / 20.0).max(0.0).ceil();
    if !n.is_finite() || n > u32::MAX as f64 {
        return Err("必要数が整数上限を超えます".into());
    }
    Ok((n as u32, lambda))
}

fn explicit_use(model: &Model, elem: &ElementData, usage: FrameSectionUse) -> bool {
    model
        .element_section(elem)
        .is_some_and(|s| Some(s.id) == elem.section && s.frame_use == Some(usage))
}

fn column_at(model: &Model, node: NodeId) -> bool {
    model.elements.iter().any(|e| {
        matches!(
            e.kind,
            ElementKind::Beam | ElementKind::Fiber | ElementKind::MultiSpring
        ) && e.nodes.len() == 2
            && e.nodes.contains(&node)
            && explicit_use(model, e, FrameSectionUse::Column)
            && model.member_length(e).is_finite()
            && model.member_length(e) > 0.0
    })
}

/// 保存groupまたは柱へ直接接続した単一大梁を確認する。失敗時に分割長へ代替しない。
pub fn resolve_lateral_bracing_reference(
    model: &Model,
    elem_id: ElemId,
) -> LateralBracingReference {
    match resolve(model, elem_id) {
        Ok(result) => result,
        Err(reason) => LateralBracingReference::Unavailable { reason },
    }
}

fn resolve(model: &Model, elem_id: ElemId) -> Result<LateralBracingReference, String> {
    let groups: Vec<_> = model
        .girder_groups
        .iter()
        .filter(|g| g.contains(&elem_id))
        .collect();
    if groups.len() > 1 {
        return Err("要素が重複groupに所属しています".into());
    }
    let ids = groups
        .first()
        .map_or_else(|| vec![elem_id], |g| (*g).clone());
    let mut parts = Vec::new();
    for id in &ids {
        if ids.iter().filter(|other| *other == id).count() != 1
            || model
                .girder_groups
                .iter()
                .filter(|g| g.contains(id))
                .count()
                > 1
        {
            return Err("group内IDまたは所属が重複しています".into());
        }
        let mut matches = model.elements.iter().filter(|e| e.id == *id);
        let e = matches.next().ok_or("group要素IDが欠落しています")?;
        if matches.next().is_some() {
            return Err("要素IDが重複しています".into());
        }
        if !matches!(
            e.kind,
            ElementKind::Beam | ElementKind::Fiber | ElementKind::MultiSpring
        ) || e.nodes.len() != 2
            || !explicit_use(model, e, FrameSectionUse::Girder)
        {
            return Err(format!("要素{}は明示した2節点大梁でありません", id.0));
        }
        for node in &e.nodes {
            if model.nodes.get(node.index()).is_none_or(|n| n.id != *node) {
                return Err("材長計算用の節点IDと添字が不整合です".into());
            }
            let n = model
                .node(*node)
                .filter(|n| n.id == *node)
                .ok_or("節点IDが欠落しています")?;
            if !n.coord.iter().all(|v| v.is_finite()) {
                return Err("節点座標が非有限です".into());
            }
        }
        positive(model.member_length(e), "節点間L")?;
        parts.push(e);
    }
    let first = *parts.first().ok_or("groupが空です")?;
    let first_start = if let Some(next) = parts.get(1) {
        match (
            next.nodes.contains(&first.nodes[0]),
            next.nodes.contains(&first.nodes[1]),
        ) {
            (false, true) => 0,
            (true, false) => 1,
            _ => return Err("group節点が一意に連続していません".into()),
        }
    } else {
        0
    };
    let mut oriented = vec![(first, first_start, 1 - first_start)];
    for e in parts.iter().skip(1) {
        let (previous, _, end_index) = oriented.last().unwrap();
        let shared = previous.nodes[*end_index];
        let start_index = if e.nodes[0] == shared {
            0
        } else if e.nodes[1] == shared {
            1
        } else {
            return Err("group節点が連続していません".into());
        };
        if column_at(model, shared) {
            return Err("group内節点に柱が接続し複数柱間です".into());
        }
        oriented.push((*e, start_index, 1 - start_index));
    }
    let (first, start_index, _) = oriented.first().unwrap();
    let (last, _, end_index) = oriented.last().unwrap();
    if first.end_cond[*start_index] != EndCondition::Fixed
        || last.end_cond[*end_index] != EndCondition::Fixed
    {
        return Err("柱間両外端Fixed/Fixed以外は対象外です".into());
    }
    if !column_at(model, first.nodes[*start_index]) || !column_at(model, last.nodes[*end_index]) {
        return Err("柱間両外端の同一NodeIdへの明示Column接続が未確認です".into());
    }
    let start = model.node(first.nodes[*start_index]).unwrap().coord;
    let end = model.node(last.nodes[*end_index]).unwrap().coord;
    let delta: [f64; 3] = std::array::from_fn(|k| end[k] - start[k]);
    let length = delta.iter().map(|v| v * v).sum::<f64>().sqrt();
    positive(length, "柱間全長L")?;
    let axis = delta.map(|v| v / length);
    for (e, start_index, end_index) in &oriented {
        let a = model.node(e.nodes[*start_index]).unwrap().coord;
        let b = model.node(e.nodes[*end_index]).unwrap().coord;
        let segment: [f64; 3] = std::array::from_fn(|k| b[k] - a[k]);
        let projection: f64 = segment.iter().zip(axis).map(|(s, d)| s * d).sum();
        let residual = segment
            .iter()
            .zip(axis)
            .map(|(s, d)| (s - projection * d).powi(2))
            .sum::<f64>()
            .sqrt();
        if !projection.is_finite()
            || projection <= 0.0
            || !residual.is_finite()
            || residual > 1e-6 * model.member_length(e)
        {
            return Err("groupが折返しまたは非共線です".into());
        }
    }
    let mut governing: Option<LateralBracingReference> = None;
    for e in parts {
        let sec = model.element_section(e).ok_or("断面が欠落しています")?;
        sec.ensure_properties_resolved()?;
        let mat = model.element_material(e).ok_or("材料が欠落しています")?;
        let (n, lambda_y) =
            count(length, sec, mat).map_err(|reason| format!("要素{}: {reason}", e.id.0))?;
        if governing.as_ref().is_none_or(|r| matches!(r, LateralBracingReference::Available { required_count, .. } if n > *required_count)) {
            governing = Some(LateralBracingReference::Available { required_count: n, length_mm: length, governing_element: e.id, section: sec.id, section_name: sec.name.clone(), material_name: mat.name.clone(), lambda_y });
        }
    }
    governing.ok_or("支配区間が未確認です".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{run_member_design_checks, CheckOutcome, MemberDesignCheckOptions};
    use sepika_core::dof::Dof6Mask;
    use sepika_core::ids::MaterialId;
    use sepika_core::model::{ForceRegime, LocalAxis, Node, RigidZone};
    use sepika_element::frame::beam::MemberForces;

    fn material(name: &str) -> Material {
        Material {
            id: MaterialId(0),
            name: name.into(),
            category: MaterialCategory::Steel,
            young: 205000.0,
            poisson: 0.3,
            density: 7.85e-9,
            shear: None,
            fc: None,
            fy: Some(235.0),
            concrete_class: Default::default(),
            strength_factor: None,
        }
    }
    fn section() -> Section {
        let mut sec = SectionShape::SteelH {
            height: 400.0,
            width: 200.0,
            web_thick: 10.0,
            flange_thick: 15.0,
            root_r: Some(0.0),
        }
        .to_section(SectionId(0), "H-reference".into());
        sec.frame_use = Some(FrameSectionUse::Girder);
        sec.material = Some(MaterialId(0));
        sec.area = 10000.0;
        sec.iz = 100_000_000.0;
        sec.iy = 300_000_000.0;
        sec
    }
    fn node(id: u32, x: f64, z: f64) -> Node {
        Node {
            id: NodeId(id),
            coord: [x, 0.0, z],
            restraint: Dof6Mask::FREE,
            mass: None,
            story: None,
            support_spring: None,
        }
    }
    fn element(id: u32, i: u32, j: u32, section: u32) -> ElementData {
        ElementData {
            id: ElemId(id),
            kind: ElementKind::Beam,
            nodes: vec![NodeId(i), NodeId(j)].into_iter().collect(),
            section: Some(SectionId(section)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 0.0, 1.0],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: RigidZone::default(),
            plastic_zone: None,
            spring: None,
        }
    }
    fn model(split: bool) -> Model {
        let mut column = section();
        column.id = SectionId(1);
        column.frame_use = Some(FrameSectionUse::Column);
        let mut model = Model {
            nodes: vec![
                node(0, 0.0, 0.0),
                node(1, 20000.0, 0.0),
                node(2, 0.0, -3000.0),
                node(3, 20000.0, -3000.0),
            ],
            sections: vec![section(), column],
            materials: vec![material("SS400")],
            elements: vec![
                element(0, 0, 1, 0),
                element(1, 2, 0, 1),
                element(2, 3, 1, 1),
            ],
            ..Default::default()
        };
        if split {
            model.nodes.push(node(4, 5000.0, 0.0));
            model.nodes.push(node(5, 15000.0, 0.0));
            model.elements[0].nodes[1] = NodeId(4);
            model.elements.push(element(3, 4, 5, 0));
            model.elements.push(element(4, 5, 1, 0));
            model.girder_groups = vec![vec![ElemId(0), ElemId(3), ElemId(4)]];
        }
        model
    }
    fn available(model: &Model, elem: u32, expected: u32, expected_elem: u32, lambda: f64) {
        match resolve_lateral_bracing_reference(model, ElemId(elem)) {
            LateralBracingReference::Available {
                required_count,
                length_mm,
                governing_element,
                lambda_y,
                ..
            } => {
                assert_eq!(required_count, expected);
                assert_eq!(length_mm, 20000.0);
                assert_eq!(governing_element, ElemId(expected_elem));
                assert!((lambda_y - lambda).abs() < 1e-9);
            }
            other => panic!("{other:?}"),
        }
    }
    fn unavailable(model: &Model, reason: &str) {
        match resolve_lateral_bracing_reference(model, ElemId(0)) {
            LateralBracingReference::Unavailable { reason: message } => {
                assert!(message.contains(reason), "{message}")
            }
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn reference_formula_independent_boundary_expectations() {
        let sec = section();
        for (name, c, at200) in [("SS400", 170.0, 2), ("SS490", 130.0, 4)] {
            for (lambda, n) in [
                (c, 0),
                (c + 1e-8, 1),
                (c + 20.0, 1),
                (c + 20.0 + 1e-8, 2),
                (200.0, at200),
            ] {
                let result = count(lambda * 100.0, &sec, &material(name)).unwrap();
                assert_eq!(result.0, n);
                assert!((result.1 - lambda).abs() < 1e-9);
            }
        }
        assert_eq!(count(9000.0, &sec, &material("SS400")).unwrap(), (0, 90.0));
        assert_eq!(
            count(25000.0, &sec, &material("SS400")).unwrap(),
            (4, 250.0)
        );
    }
    #[test]
    fn reference_material_allowlist_does_not_classify_by_f() {
        for name in [
            "SS400", "SN400", "SN400A", "SN400B", "SN400C", "SM400", "SM400A", "SM400B", "SM400C",
            "SS490", "SN490", "SN490B", "SN490C", "SM490", "SM490A", "SM490B", "SM490C", "SM490YA",
            "SM490YB",
        ] {
            let mut mat = material(name);
            mat.fy = None;
            let n = if name.contains("400") { 2 } else { 4 };
            assert_eq!(count(20000.0, &section(), &mat).unwrap().0, n, "{name}");
            let mut thick = section();
            if let Some(SectionShape::SteelH { flange_thick, .. }) = &mut thick.shape {
                *flange_thick = 50.0;
            }
            assert_eq!(count(20000.0, &thick, &mat).unwrap().0, n, "{name}");
        }
        for name in [
            "FY235", "SS400XYZ", "SN490A", "SM400YA", "SM400YB", "BCP235", "BCR235", "TMCP325",
            "SM520", "SN400ABC",
        ] {
            assert!(
                count(20000.0, &section(), &material(name))
                    .unwrap_err()
                    .contains("対象外"),
                "{name}"
            );
        }
    }
    #[test]
    fn reference_invalid_numbers_shapes_and_strengths_are_never_zero() {
        for value in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            for property in 0..3 {
                let mut sec = section();
                let length = match property {
                    0 => {
                        sec.area = value;
                        20000.0
                    }
                    1 => {
                        sec.iz = value;
                        20000.0
                    }
                    _ => value,
                };
                assert!(count(length, &sec, &material("SS400")).is_err());
            }
            let mut mat = material("SS400");
            mat.fy = Some(value);
            assert!(count(20000.0, &section(), &mat).is_err());
            let mut sec = section();
            if let Some(SectionShape::SteelH { height, .. }) = &mut sec.shape {
                *height = value;
            }
            assert!(count(20000.0, &sec, &material("SS400")).is_err());
        }
        assert!(count(1e15, &section(), &material("SS400"))
            .unwrap_err()
            .contains("整数上限"));
        let mut sec = section();
        sec.area = f64::MIN_POSITIVE;
        sec.iz = f64::MAX;
        sec.iy = f64::MAX;
        assert!(count(20000.0, &sec, &material("SS400")).is_err());
        let mut sec = section();
        sec.iz = sec.iy * 2.0;
        assert!(count(20000.0, &sec, &material("SS400"))
            .unwrap_err()
            .contains("Iz規約"));
        let mut sec = section();
        sec.shape = None;
        assert!(count(20000.0, &sec, &material("SS400"))
            .unwrap_err()
            .contains("SteelH"));
        sec.shape = Some(SectionShape::SteelBox {
            height: 400.0,
            width: 200.0,
            thick: 10.0,
            corner_r: None,
        });
        assert!(count(20000.0, &sec, &material("SS400")).is_err());
        sec.shape = Some(SectionShape::SteelBuiltH {
            height: 400.0,
            upper_width: 200.0,
            lower_width: 200.0,
            web_thick: 10.0,
            upper_thick: 15.0,
            lower_thick: 15.0,
        });
        assert!(count(20000.0, &sec, &material("SS400")).is_err());
        sec.shape = Some(SectionShape::SteelTee {
            height: 400.0,
            width: 200.0,
            web_thick: 10.0,
            flange_thick: 15.0,
        });
        assert!(count(20000.0, &sec, &material("SS400")).is_err());
        let mut m = model(false);
        m.sections[0].frame_use = None;
        unavailable(&m, "大梁");
        let mut m = model(false);
        m.elements[0].end_cond[1] = EndCondition::Pinned;
        unavailable(&m, "Fixed");
        m.elements[0].end_cond[1] = EndCondition::SemiRigid { k_theta: 1e8 };
        unavailable(&m, "Fixed");
    }
    #[test]
    fn reference_uses_column_node_connections_and_full_group_length() {
        available(&model(false), 0, 2, 0, 200.0);
        let mut m = model(true);
        available(&m, 0, 2, 0, 200.0);
        available(&m, 3, 2, 0, 200.0);
        available(&m, 4, 2, 0, 200.0);
        for id in [0, 3, 4] {
            m.elements[id].nodes.swap(0, 1);
        }
        available(&m, 0, 2, 0, 200.0);
        m.elements[0].end_cond[1] = EndCondition::Pinned;
        unavailable(&m, "Fixed");
        m.elements[0].end_cond[1] = EndCondition::Fixed;
        m.girder_groups.clear();
        unavailable(&m, "Column");
        let mut m = model(false);
        m.elements[2].section = Some(SectionId(0));
        unavailable(&m, "Column");
        let mut m = model(false);
        m.nodes.push(node(4, 20000.0, 0.0));
        m.elements[2].nodes[1] = NodeId(4);
        unavailable(&m, "Column");
    }
    #[test]
    fn reference_governing_segment_keeps_section_material_and_lambda_together_including_ties() {
        for governing in [0, 3, 4] {
            let mut m = model(true);
            let mut sec = section();
            sec.id = SectionId(2);
            sec.name = format!("governing-{governing}");
            sec.iz = 25_000_000.0;
            m.sections.push(sec);
            m.elements[governing as usize].section = Some(SectionId(2));
            available(&m, 0, 12, governing, 400.0);
            match resolve_lateral_bracing_reference(&m, ElemId(3)) {
                LateralBracingReference::Available {
                    section,
                    section_name,
                    material_name,
                    lambda_y,
                    ..
                } => {
                    assert_eq!(section, SectionId(2));
                    assert_eq!(section_name, format!("governing-{governing}"));
                    assert_eq!(material_name, "SS400");
                    assert_eq!(lambda_y, 400.0);
                }
                other => panic!("{other:?}"),
            }
        }
        for governing in [0, 3, 4] {
            let mut m = model(true);
            let mut sec = section();
            sec.id = SectionId(2);
            sec.material = Some(MaterialId(1));
            m.sections.push(sec);
            m.materials.push(Material {
                id: MaterialId(1),
                ..material("SN490B")
            });
            m.elements[governing].section = Some(SectionId(2));
            available(&m, 0, 4, governing as u32, 200.0);
            assert!(
                matches!(resolve_lateral_bracing_reference(&m, ElemId(0)), LateralBracingReference::Available { material_name, .. } if material_name == "SN490B")
            );
        }
        let mut m = model(true);
        let mut sec = section();
        sec.id = SectionId(2);
        sec.iz = 95_000_000.0;
        m.sections.push(sec);
        m.elements[3].section = Some(SectionId(2));
        available(&m, 0, 2, 0, 200.0);
        m.materials.push(Material {
            id: MaterialId(1),
            ..material("FY235")
        });
        m.sections[2].material = Some(MaterialId(1));
        unavailable(&m, "材種FY235");
    }
    #[test]
    fn reference_invalid_groups_cannot_fall_back_to_single_member() {
        let mut m = model(true);
        m.girder_groups[0].push(ElemId(999));
        unavailable(&m, "欠落");
        let mut m = model(true);
        m.girder_groups[0].push(ElemId(0));
        unavailable(&m, "重複");
        let mut m = model(true);
        m.girder_groups.push(vec![ElemId(3), ElemId(4)]);
        unavailable(&m, "重複");
        let mut m = model(true);
        m.girder_groups[0].swap(1, 2);
        unavailable(&m, "連続");
        let mut m = model(true);
        m.nodes[4].coord[1] = 10.0;
        unavailable(&m, "非共線");
        let mut m = model(true);
        m.nodes[5].coord[0] = 4000.0;
        unavailable(&m, "折返し");
        let mut m = model(true);
        m.nodes[5].coord[0] = f64::NAN;
        unavailable(&m, "非有限");
        let mut m = model(true);
        m.elements.push(element(5, 2, 4, 1));
        unavailable(&m, "複数柱間");
        let mut m = model(true);
        m.elements[4].end_cond[1] = EndCondition::Pinned;
        unavailable(&m, "Fixed");
        let mut m = model(true);
        m.elements.push(m.elements[3].clone());
        unavailable(&m, "IDが重複");
    }
    #[test]
    fn reference_shared_check_entry_preserves_bending_shear_and_lb() {
        let mut m = model(true);
        let forces = vec![(
            ElemId(0),
            MemberForces {
                at: vec![
                    (0.0, [0.0; 6]),
                    (0.5, [0.0, 1000.0, 0.0, 0.0, 0.0, 1e6]),
                    (1.0, [0.0; 6]),
                ],
            },
        )];
        let report =
            run_member_design_checks(&m, &forces, &[], &MemberDesignCheckOptions::default())
                .unwrap();
        assert_eq!(
            report.lateral_bracing_references,
            vec![(ElemId(0), resolve_lateral_bracing_reference(&m, ElemId(0)))]
        );
        let checked: Vec<_> = report
            .member_checks
            .iter()
            .map(|(_, _, o)| {
                if let CheckOutcome::Checked(r) = o {
                    assert!(r.detail.contains("（参考）n=2"));
                    assert!(!r.detail.contains("端条件未確認"));
                    assert!(r.detail.contains("実配置・補剛性能は未検定"));
                    assert_eq!(r.components.len(), 2);
                    r.components.iter().map(|c| c.ratio).collect::<Vec<_>>()
                } else {
                    panic!("{o:?}")
                }
            })
            .collect();
        m.elements[4].end_cond[1] = EndCondition::Pinned;
        let unavailable_report =
            run_member_design_checks(&m, &forces, &[], &MemberDesignCheckOptions::default())
                .unwrap();
        for ((_, _, outcome), ratios) in unavailable_report.member_checks.iter().zip(checked) {
            let CheckOutcome::Checked(result) = outcome else {
                panic!("{outcome:?}")
            };
            assert!(result.detail.contains("算定不能"));
            assert_eq!(
                result
                    .components
                    .iter()
                    .map(|c| c.ratio)
                    .collect::<Vec<_>>(),
                ratios
            );
        }
        m.elements[4].end_cond[1] = EndCondition::Fixed;
        let reference = resolve_lateral_bracing_reference(&m, ElemId(0));
        assert!(matches!(
            reference,
            LateralBracingReference::Available {
                required_count: 2,
                ..
            }
        ));
        m.steel_design_attrs
            .push(sepika_core::model::SteelDesignAttr {
                elem: ElemId(0),
                lateral_brace_count: Some(100),
                joint_flange_loss: 0.0,
                joint_web_loss: 0.0,
                scallop_web_loss: 0.0,
                lb_direct: None,
                lk_y_direct: None,
                lk_z_direct: None,
                c_direct: None,
            });
        assert_eq!(resolve_lateral_bracing_reference(&m, ElemId(0)), reference);
        m.steel_design_attrs[0].lb_direct = Some((100.0, 200.0, 300.0));
        assert_eq!(resolve_lateral_bracing_reference(&m, ElemId(0)), reference);
    }
    #[test]
    fn sm520_out_of_range_lateral_bracing_public_resolver_keeps_reason_before_fy_fallback() {
        for name in ["SM520", "SM520B", "SM520C"] {
            for fy in [None, Some(177.0), Some(235.0), Some(335.0), Some(999.0)] {
                let mut model = model(false);
                model.materials[0].name = name.into();
                model.materials[0].fy = fy;
                model.sections[0].shape = Some(SectionShape::SteelH {
                    height: 1000.0,
                    width: 400.0,
                    web_thick: 100.001,
                    flange_thick: 80.0,
                    root_r: Some(0.0),
                });
                unavailable(&model, "適用範囲");
            }
        }
    }
}
