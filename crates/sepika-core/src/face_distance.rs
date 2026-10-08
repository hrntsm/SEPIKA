//! 柱フェース距離（節点から部材フェースまでの距離）の算定。

use crate::adjacency::NodeAdjacency;
use crate::geom::{element_axis as elem_axis, vec3, ORTHOGONAL_DOT_MAX};
use crate::model::{
    ElementData, FrameSectionUse, SecondaryMember, SecondaryMemberEnds, SecondaryMemberKind,
    Section, SupportMemberId,
};
use crate::model::{ElementKind, Model};
use crate::section_shape::face_geometry::SectionGeometry;

fn axis_direction(a: [f64; 3], b: [f64; 3]) -> Result<[f64; 3], String> {
    let delta = vec3::sub(b, a);
    let len = vec3::norm(delta);
    if !len.is_finite() || len <= 1e-12 {
        return Err("材軸長が不正です".into());
    }
    Ok(vec3::scale(delta, 1.0 / len))
}

fn section_frame(axis: [f64; 3], reference: [f64; 3]) -> Result<[[f64; 3]; 2], String> {
    let rejected = vec3::sub(reference, vec3::scale(axis, vec3::dot(reference, axis)));
    let norm = vec3::norm(rejected);
    if !norm.is_finite() || norm <= 1e-12 {
        return Err("断面方向を材軸直交面へ投影できません".into());
    }
    let y = vec3::scale(rejected, 1.0 / norm);
    let z = vec3::cross(axis, y);
    Ok([y, z])
}

fn section_geometry(sec: &Section) -> Result<SectionGeometry, String> {
    let shape = sec.shape.as_ref().ok_or_else(|| {
        format!(
            "断面 {} の実形状が未定義です（せい {} mm・幅 {} mm だけでは外形を解決できません）",
            sec.id.0, sec.depth, sec.width
        )
    })?;
    match shape {
        crate::section_shape::SectionShape::CftBox { height, width, .. } => {
            SectionGeometry::of(&crate::section_shape::SectionShape::SteelFlatBar {
                width: *width,
                thick: *height,
            })
        }
        crate::section_shape::SectionShape::CftPipe { outer_dia, .. } => {
            SectionGeometry::of(&crate::section_shape::SectionShape::SteelRoundBar {
                dia: *outer_dia,
            })
        }
        _ => SectionGeometry::of(shape),
    }
}

fn section_face(
    sec: &Section,
    axis: [f64; 3],
    reference: [f64; 3],
    direction: [f64; 3],
) -> Result<f64, String> {
    let [y, z] = section_frame(axis, reference)?;
    let geometry = section_geometry(sec)?;
    geometry.face_distance([vec3::dot(direction, y), vec3::dot(direction, z)])
}

fn beam_projected_face(
    sec: &Section,
    axis: [f64; 3],
    reference: [f64; 3],
    direction: [f64; 3],
) -> Result<f64, String> {
    let horizontal = axis[0].hypot(axis[1]);
    if horizontal <= 1e-12 {
        return Err("支持梁の平面内材軸を解決できません".into());
    }
    let normal = [-axis[1] / horizontal, axis[0] / horizontal, 0.0];
    let dot = vec3::dot(normal, direction);
    if !dot.is_finite() || dot.abs() <= 1e-12 {
        return Err("小梁材軸が支持梁の投影側面と平行です".into());
    }
    let [y, z] = section_frame(axis, reference)?;
    let [min, max] =
        section_geometry(sec)?.projected_range([vec3::dot(normal, y), vec3::dot(normal, z)])?;
    let distance = if dot > 0.0 { max } else { min } / dot;
    if !distance.is_finite() || distance <= 0.0 {
        return Err("支持梁の投影外側フェース距離が不正です".into());
    }
    Ok(distance)
}

fn checked_interval(label: &str, len: f64, deductions: [f64; 2]) -> Result<[f64; 2], String> {
    let [i, j] = deductions;
    if !len.is_finite() || !i.is_finite() || !j.is_finite() || len - i - j <= 0.0 {
        return Err(format!(
            "{label} の自重算定長が不正です（芯々長 {len} mm、始端控除 {i} mm、終端控除 {j} mm）"
        ));
    }
    Ok([i, len - j])
}

/// 大梁自重専用の柱面間区間 [mm]。接続柱の距離が複数ある端では最小を採る。
/// 剛域用の `face_distances` には影響しない。
pub fn girder_self_weight_interval(model: &Model, elem: &ElementData) -> Result<[f64; 2], String> {
    let label = format!("大梁 {}", elem.id.0);
    let ends = [elem.nodes.first(), elem.nodes.last()];
    let [Some(i), Some(j)] = ends else {
        return Err(format!("{label} の節点を解決できません"));
    };
    let a = model
        .node(*i)
        .ok_or_else(|| format!("{label} の始端を解決できません"))?
        .coord;
    let b = model
        .node(*j)
        .ok_or_else(|| format!("{label} の終端を解決できません"))?
        .coord;
    let direction = axis_direction(a, b)?;
    let mut deductions = [0.0; 2];
    for (end, node) in [*i, *j].into_iter().enumerate() {
        let mut minimum = f64::INFINITY;
        for column in &model.elements {
            if column.id == elem.id
                || !column.kind.is_weight_frame()
                || !column.nodes.contains(&node)
            {
                continue;
            }
            let sec = model.element_section(column).ok_or_else(|| {
                format!("{label} の接続部材 {} の断面を解決できません", column.id.0)
            })?;
            if sec.frame_use != Some(FrameSectionUse::Column) {
                continue;
            }
            let [Some(ci), Some(cj)] = [column.nodes.first(), column.nodes.last()] else {
                return Err(format!("柱 {} の節点を解決できません", column.id.0));
            };
            let ca = model
                .node(*ci)
                .ok_or_else(|| format!("柱 {} の節点を解決できません", column.id.0))?
                .coord;
            let cb = model
                .node(*cj)
                .ok_or_else(|| format!("柱 {} の節点を解決できません", column.id.0))?
                .coord;
            let distance = section_face(
                sec,
                axis_direction(ca, cb)?,
                column.local_axis.ref_vector,
                vec3::scale(direction, if end == 0 { 1.0 } else { -1.0 }),
            )
            .map_err(|error| format!("{label} の端 {}・柱 {}: {error}", end, column.id.0))?;
            minimum = minimum.min(distance);
        }
        if minimum.is_finite() {
            deductions[end] = minimum;
        }
    }
    checked_interval(&label, vec3::dist(a, b), deductions)
}

/// 二次部材の自重作用区間 [mm]。間柱・Detached は芯々区間を保持する。
/// 小梁の断面方向は鉛直を材軸直交面へ投影して決める。
pub fn secondary_self_weight_interval(
    model: &Model,
    member: &SecondaryMember,
) -> Result<[f64; 2], String> {
    let label = format!("二次部材 {}", member.id.0);
    let (a, b, len) = model
        .secondary_member_axis(member)
        .ok_or_else(|| format!("{label} の材軸を解決できません"))?;
    if member.kind != SecondaryMemberKind::Beam || member.is_detached() {
        return checked_interval(&label, len, [0.0; 2]);
    }
    let direction = axis_direction(a, b)?;
    let anchors = match member.ends {
        SecondaryMemberEnds::Supported([a, b]) => [Some(a), Some(b)],
        SecondaryMemberEnds::Cantilever { support, .. } => [Some(support), None],
        SecondaryMemberEnds::Detached(_) => unreachable!(),
    };
    let mut deductions = [0.0; 2];
    for (end, anchor) in anchors.into_iter().enumerate() {
        let Some(anchor) = anchor else {
            continue;
        };
        let (sa, sb) = model.support_member_axis(anchor.support).ok_or_else(|| {
            format!(
                "{label} の支持材 {:?} の材軸を解決できません",
                anchor.support
            )
        })?;
        let (section, reference) = match anchor.support {
            SupportMemberId::Primary(id) => {
                let element = model
                    .element(id)
                    .ok_or_else(|| format!("支持材 {} がありません", id.0))?;
                (
                    model.element_section(element),
                    element.local_axis.ref_vector,
                )
            }
            SupportMemberId::Secondary(id) => {
                let support = model
                    .secondary_member(id)
                    .ok_or_else(|| format!("支持二次部材 {} がありません", id.0))?;
                (
                    support
                        .section
                        .and_then(|id| model.sections.get(id.index())),
                    [0.0, 0.0, 1.0],
                )
            }
        };
        let section = section.ok_or_else(|| {
            format!(
                "{label} の支持材 {:?} の断面を解決できません",
                anchor.support
            )
        })?;
        deductions[end] = beam_projected_face(
            section,
            axis_direction(sa, sb)?,
            reference,
            vec3::scale(direction, if end == 0 { 1.0 } else { -1.0 }),
        )
        .map_err(|error| format!("{label} の端 {end}・支持材 {:?}: {error}", anchor.support))?;
    }
    checked_interval(&label, len, deductions)
}

/// 節点 `node` で対象部材と概ね直交する Beam 要素の最大せいの半分 [mm]。
/// 直交材がない端は 0.0。
fn face_at(
    model: &Model,
    node: crate::ids::NodeId,
    target_axis: [f64; 3],
    target_elem_idx: usize,
    adjacency: &NodeAdjacency,
) -> f64 {
    let mut d_max = 0.0_f64;
    for &ei in adjacency.indices_at(node) {
        if ei == target_elem_idx {
            continue;
        }
        let e = &model.elements[ei];
        if e.kind != ElementKind::Beam {
            continue;
        }
        let axis = elem_axis(model, e);
        if vec3::dot(axis, target_axis).abs() >= ORTHOGONAL_DOT_MAX {
            continue;
        }
        if let Some(sec) = e.section.and_then(|sid| model.sections.get(sid.index())) {
            d_max = d_max.max(sec.depth);
        }
    }
    d_max / 2.0
}

/// モデルの全要素について、両端の柱フェース距離 `[i 端, j 端]` [mm] を求める。
///
/// 添字は `model.elements` の並びと一致する。Beam 以外の要素と、節点が 2 つ
/// 未満の要素は `[0.0, 0.0]`。
pub fn face_distances(model: &Model) -> Vec<[f64; 2]> {
    let adjacency = NodeAdjacency::build(model);
    model
        .elements
        .iter()
        .enumerate()
        .map(|(i, e)| {
            if e.kind != ElementKind::Beam || e.nodes.len() < 2 {
                return [0.0, 0.0];
            }
            let axis = elem_axis(model, e);
            let ni = e.nodes[0];
            let nj = e.nodes[e.nodes.len() - 1];
            [
                face_at(model, ni, axis, i, &adjacency),
                face_at(model, nj, axis, i, &adjacency),
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{ElemId, MaterialId, NodeId, SectionId};
    use crate::model::{
        ElementData, EndCondition, ForceRegime, LocalAxis, Node, RigidZone, Section,
    };

    fn node(id: u32, c: [f64; 3]) -> Node {
        Node {
            id: NodeId(id),
            coord: c,
            restraint: Default::default(),
            mass: None,
            story: None,
            support_spring: None,
        }
    }

    fn section(id: u32, depth: f64) -> Section {
        Section {
            id: SectionId(id),
            name: String::new(),
            frame_use: None,
            area: 0.0,
            iy: 0.0,
            iz: 0.0,
            j: 0.0,
            depth,
            width: 0.0,
            as_y: 0.0,
            as_z: 0.0,
            floor: None,
            panel_thickness: None,
            thickness: None,
            shape: None,
            material: Some(MaterialId(0)),
            rebar_material: None,
            shear_rebar_material: None,
            steel_material: None,
            property_basis: Default::default(),
        }
    }

    fn elem(id: u32, kind: ElementKind, a: u32, b: u32, sec: u32) -> ElementData {
        ElementData {
            id: ElemId(id),
            kind,
            nodes: [NodeId(a), NodeId(b)].into_iter().collect(),
            section: Some(SectionId(sec)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 0.0, 1.0],
            },
            end_cond: [EndCondition::Fixed, EndCondition::Fixed],
            force_regime: ForceRegime::Auto,
            rigid_zone: RigidZone::default(),
            plastic_zone: None,
            spring: None,
        }
    }

    #[test]
    fn 自重大梁は柱だけを控除し上下柱の小さい面距離を使う() {
        let mut model = Model {
            nodes: vec![
                node(0, [0.0, 0.0, 0.0]),
                node(1, [6000.0, 0.0, 0.0]),
                node(2, [0.0, 0.0, -3000.0]),
                node(3, [6000.0, 0.0, -3000.0]),
                node(4, [0.0, 4000.0, 0.0]),
                node(5, [0.0, 0.0, 3000.0]),
            ],
            elements: vec![
                elem(0, ElementKind::Beam, 0, 1, 0),
                elem(1, ElementKind::Beam, 2, 0, 1),
                elem(2, ElementKind::Beam, 3, 1, 1),
                elem(3, ElementKind::Beam, 0, 4, 2),
            ],
            sections: vec![
                section(0, 700.0),
                section(1, 600.0),
                section(2, 900.0),
                section(3, 800.0),
            ],
            ..Default::default()
        };
        for s in &mut model.sections {
            s.width = s.depth;
            s.shape = Some(crate::section_shape::SectionShape::SteelFlatBar {
                width: s.width,
                thick: s.depth,
            });
        }
        model.sections[0].frame_use = Some(FrameSectionUse::Girder);
        model.sections[1].frame_use = Some(FrameSectionUse::Column);
        model.sections[2].frame_use = Some(FrameSectionUse::Girder);
        model.sections[3].frame_use = Some(FrameSectionUse::Column);
        for e in &mut model.elements[1..3] {
            e.local_axis.ref_vector = [1.0, 0.0, 0.0];
        }
        assert_eq!(
            girder_self_weight_interval(&model, &model.elements[0]).unwrap(),
            [300.0, 5700.0]
        );
        for kind in [ElementKind::Fiber, ElementKind::MultiSpring] {
            let mut mixed = model.clone();
            mixed.elements[0].kind = kind;
            mixed.elements[1].kind = kind;
            assert_eq!(
                girder_self_weight_interval(&mixed, &mixed.elements[0]).unwrap(),
                [300.0, 5700.0]
            );
        }
        let mut upper = elem(4, ElementKind::Beam, 0, 5, 3);
        upper.local_axis.ref_vector = [1.0, 0.0, 0.0];
        model.elements.push(upper);
        assert_eq!(
            girder_self_weight_interval(&model, &model.elements[0]).unwrap(),
            [300.0, 5700.0]
        );
        model.sections[3].depth = 400.0;
        model.sections[3].shape = Some(crate::section_shape::SectionShape::SteelFlatBar {
            width: 800.0,
            thick: 400.0,
        });
        assert_eq!(
            girder_self_weight_interval(&model, &model.elements[0]).unwrap(),
            [200.0, 5700.0]
        );
        model.elements.remove(2);
        assert_eq!(
            girder_self_weight_interval(&model, &model.elements[0]).unwrap(),
            [200.0, 6000.0]
        );
        model.sections[3].depth = 14000.0;
        model.sections[1].depth = 14000.0;
        for index in [1, 3] {
            let s = &mut model.sections[index];
            s.shape = Some(crate::section_shape::SectionShape::SteelFlatBar {
                width: s.width,
                thick: s.depth,
            });
        }
        let error = girder_self_weight_interval(&model, &model.elements[0]).unwrap_err();
        assert!(
            error.contains("大梁 0") && error.contains("7000") && error.contains("6000"),
            "{error}"
        );
    }

    #[test]
    fn 断面方向と斜め取付きは実フェース交点を使う() {
        let mut s = section(0, 600.0);
        s.width = 400.0;
        s.shape = Some(crate::section_shape::SectionShape::SteelFlatBar {
            width: 400.0,
            thick: 600.0,
        });
        let d = std::f64::consts::FRAC_1_SQRT_2;
        let distance = section_face(&s, [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [d, d, 0.0]).unwrap();
        assert!((distance - 200.0 / d).abs() < 1e-9);
        assert_eq!(
            section_face(&s, [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [1.0, 0.0, 0.0]).unwrap(),
            300.0
        );
        assert_eq!(
            section_face(&s, [0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]).unwrap(),
            200.0
        );
    }

    #[test]
    fn 小梁はhのフランジと非対称channelの投影外側フェースを使う() {
        use crate::section_shape::SectionShape;
        let mut s = section(0, 300.0);
        s.width = 200.0;
        s.shape = Some(SectionShape::SteelH {
            root_r: Some(0.0),
            height: 300.0,
            width: 200.0,
            web_thick: 10.0,
            flange_thick: 20.0,
        });
        let face = |s: &Section, direction| {
            beam_projected_face(s, [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], direction).unwrap()
        };
        assert_eq!(face(&s, [0.0, 1.0, 0.0]), 100.0);
        let d = std::f64::consts::FRAC_1_SQRT_2;
        assert!((face(&s, [d, d, 0.0]) - 100.0 / d).abs() < 1e-9);
        s.shape = Some(SectionShape::SteelChannel {
            height: 300.0,
            width: 100.0,
            web_thick: 10.0,
            flange_thick: 20.0,
        });
        let centroid = (4000.0 * 50.0 + 2600.0 * 5.0) / 6600.0;
        assert!((face(&s, [0.0, 1.0, 0.0]) - centroid).abs() < 1e-9);
        assert!((face(&s, [0.0, -1.0, 0.0]) - (100.0 - centroid)).abs() < 1e-9);
        assert!(
            (beam_projected_face(&s, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 1.0, 0.0]).unwrap()
                - 150.0)
                .abs()
                < 1e-9
        );
        s.shape = None;
        assert!(face_error(&s).contains("実形状が未定義"));
        fn face_error(s: &Section) -> String {
            beam_projected_face(s, [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]).unwrap_err()
        }
    }

    #[test]
    fn 円形柱は半径と大梁軸の交点であり平面投影幅へ読み替えない() {
        let mut s = section(0, 600.0);
        s.shape = Some(crate::section_shape::SectionShape::SteelRoundBar { dia: 600.0 });
        let direction = [0.8, 0.0, 0.6];
        assert!(
            (section_face(&s, [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], direction).unwrap() - 375.0).abs()
                < 1e-9
        );
    }

    #[test]
    fn 勾配かつ回転した支持梁は断面姿勢の投影と取付き三次元方向を使う() {
        let mut s = section(0, 600.0);
        s.shape = Some(crate::section_shape::SectionShape::SteelFlatBar {
            width: 300.0,
            thick: 600.0,
        });
        let norm = 1.64_f64.sqrt();
        let expected_q = 300.0 / norm + 150.0 * 0.8 / norm;
        let distance =
            beam_projected_face(&s, [0.8, 0.0, 0.6], [0.0, 1.0, 1.0], [0.0, 0.6, 0.8]).unwrap();
        assert!((distance - expected_q / 0.6).abs() < 1e-9);
        let rolled =
            beam_projected_face(&s, [1.0, 0.0, 0.0], [0.0, 1.0, 1.0], [0.0, 1.0, 0.0]).unwrap();
        assert!((rolled - 450.0 / 2.0_f64.sqrt()).abs() < 1e-9);
    }

    /// 柱（せい 600）が取り付く端のフェース距離は柱せいの半分、直交材がない端は 0。
    #[test]
    fn 直交材のせいの半分をフェース距離とする() {
        let model = Model {
            nodes: vec![
                node(0, [0.0, 0.0, 0.0]),
                node(1, [0.0, 0.0, 3000.0]),
                node(2, [4000.0, 0.0, 3000.0]),
            ],
            elements: vec![
                elem(0, ElementKind::Beam, 0, 1, 0),
                elem(1, ElementKind::Beam, 1, 2, 1),
            ],
            sections: vec![section(0, 600.0), section(1, 700.0)],
            ..Default::default()
        };
        let f = face_distances(&model);
        // 梁（要素 1）: i 端に柱が取り付くので 600/2、j 端は直交材なしで 0。
        assert_eq!(f[1], [300.0, 0.0]);
        // 柱（要素 0）: 上端に梁が取り付くので 700/2、下端は直交材なしで 0。
        assert_eq!(f[0], [0.0, 350.0]);
    }

    /// フェース距離を決めるのは柱・大梁だけで、壁は数えない。
    ///
    /// 壁を数えると、剛域長を求めるときの「部材フェース」と食い違う。
    #[test]
    fn 壁はフェース距離に数えない() {
        let mut model = Model {
            nodes: vec![
                node(0, [0.0, 0.0, 0.0]),
                node(1, [0.0, 0.0, 3000.0]),
                node(2, [4000.0, 0.0, 3000.0]),
            ],
            elements: vec![elem(0, ElementKind::Beam, 1, 2, 0)],
            sections: vec![section(0, 700.0), section(1, 9999.0)],
            ..Default::default()
        };
        assert_eq!(face_distances(&model)[0], [0.0, 0.0]);

        // 梁の i 端に直交する壁を足しても変わらない。
        model.elements.push(elem(1, ElementKind::Wall, 0, 1, 1));
        assert_eq!(face_distances(&model)[0], [0.0, 0.0]);

        // 同じ位置に柱（Beam）を足すと、そのせいの半分が効く。
        model.elements.push(elem(2, ElementKind::Beam, 0, 1, 1));
        assert_eq!(face_distances(&model)[0], [4999.5, 0.0]);
    }
}
