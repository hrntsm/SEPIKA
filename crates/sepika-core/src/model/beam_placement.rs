use super::*;
use crate::geom::vec3::{add, dist, dot, local_frame_axes, sub};
use crate::section_shape::face_geometry::{Primitive, SectionGeometry};

/// 梁の有限な実配置。座標・偏心・断面寸法は mm、基底は局所 x/y/z 順。
pub struct BeamPlacement {
    pub member: SupportMemberId,
    pub reference_ends: [[f64; 3]; 2],
    pub vertical_eccentricity: f64,
    pub actual_ends: [[f64; 3]; 2],
    pub axes: [[f64; 3]; 3],
    pub section_shape: crate::section_shape::SectionShape,
    geometry: SectionGeometry,
    length_mm: f64,
}

impl Model {
    /// 大梁・小梁の実配置を導出する。未指定・不正な偏心、位置、断面は対象と理由付きで失敗する。
    pub fn beam_placement(&self, member: SupportMemberId) -> Result<BeamPlacement, String> {
        let resolve = || {
            let (ends, eccentricity, section, reference) = match member {
                SupportMemberId::Primary(id) => {
                    let e = self.element(id).ok_or("部材がありません")?;
                    if !e.kind.is_primary_frame_support() || e.nodes.len() != 2 {
                        return Err("大梁の二節点線材ではありません".to_string());
                    }
                    let section = self
                        .section(e.section.ok_or("断面が未指定です")?)
                        .ok_or("断面がありません")?;
                    if section.frame_use != Some(FrameSectionUse::Girder) {
                        return Err("断面用途が大梁ではありません".to_string());
                    }
                    let a = self.node(e.nodes[0]).ok_or("i端節点がありません")?.coord;
                    let b = self.node(e.nodes[1]).ok_or("j端節点がありません")?.coord;
                    (
                        [a, b],
                        e.vertical_eccentricity,
                        section,
                        e.local_axis.ref_vector,
                    )
                }
                SupportMemberId::Secondary(id) => {
                    let sm = self.secondary_member(id).ok_or("小梁がありません")?;
                    if sm.kind != SecondaryMemberKind::Beam || sm.is_detached() {
                        return Err("支持アンカーを持つ小梁が必要です".to_string());
                    }
                    self.validate_beam_reference_supports(member)?;
                    let (a, b) = self
                        .secondary_member_end_points(sm)
                        .ok_or("支持アンカーの基準両端位置を解決できません")?;
                    let section = self
                        .section(sm.section.ok_or("断面が未指定です")?)
                        .ok_or("断面がありません")?;
                    ([a, b], sm.vertical_eccentricity, section, [0.0, 0.0, 1.0])
                }
            };
            BeamPlacement::new(member, ends, eccentricity, section, reference)
        };
        resolve().map_err(|reason| format!("{} の実配置: {reason}", beam_label(member)))
    }

    /// 梁用途の主架構の実材軸端 [mm] を返す。対象外は None、未指定・不正な端位置や偏心は理由付きで失敗する。
    pub fn primary_beam_axis_ends(
        &self,
        data: &ElementData,
    ) -> Result<Option<[[f64; 3]; 2]>, String> {
        if !data.kind.is_primary_frame_support() {
            return Ok(None);
        }
        let fail = |end: &str, item: &str, reason: &str| {
            format!("部材 ID {} {end}の{item}: {reason}", data.id.0)
        };
        let section = self
            .element_section(data)
            .ok_or_else(|| fail("i/j端", "断面", "未指定または参照先がありません"))?;
        if section.frame_use != Some(FrameSectionUse::Girder) {
            return Ok(None);
        }
        if data.nodes.len() != 2 {
            return Err(fail("i/j端", "節点", "二節点が必要です"));
        }
        let eccentricity = data
            .vertical_eccentricity
            .ok_or_else(|| fail("i/j端", "鉛直偏心", "未指定です"))?;
        if !eccentricity.is_finite() {
            return Err(fail("i/j端", "鉛直偏心", "非有限です"));
        }
        let mut ends = [[0.0; 3]; 2];
        for (side, name) in ["i端", "j端"].iter().enumerate() {
            let node = self
                .node(data.nodes[side])
                .ok_or_else(|| fail(name, "節点", "存在しません"))?;
            if !node.coord.iter().all(|v| v.is_finite()) {
                return Err(fail(name, "接続節点座標", "非有限です"));
            }
            ends[side] = node.coord;
        }
        let ends = actual_beam_axis_ends(ends, eccentricity)
            .map_err(|reason| format!("部材 ID {} {reason}", data.id.0))?;
        if !dist(ends[0], ends[1]).is_finite() || dist(ends[0], ends[1]) <= 0.0 {
            return Err(fail("i/j端", "実材軸長", "有限な正値ではありません"));
        }
        local_frame_axes(ends[0], ends[1], data.local_axis.ref_vector)
            .map_err(|reason| fail("i/j端", "実材軸・局所軸", reason))?;
        Ok(Some(ends))
    }

    fn validate_beam_reference_supports(&self, member: SupportMemberId) -> Result<(), String> {
        let mut pending = vec![member];
        let mut checked = std::collections::HashSet::new();
        while let Some(support) = pending.pop() {
            if !checked.insert(support) {
                continue;
            }
            match support {
                SupportMemberId::Primary(id) => {
                    let e = self.element(id).ok_or("支持部材がありません")?;
                    if !e.kind.is_primary_frame_support() || e.nodes.len() != 2 {
                        return Err(format!(
                            "支持部材 ID {} が二節点主架構線材ではありません",
                            id.0
                        ));
                    }
                }
                SupportMemberId::Secondary(id) => {
                    let sm = self
                        .secondary_member(id)
                        .ok_or("支持二次部材がありません")?;
                    if sm.is_detached() {
                        return Err(format!("支持二次部材 ID {} のアンカーが未解決です", id.0));
                    }
                    for anchor in sm.ends.anchors() {
                        if !anchor.position.is_finite() || !(0.0..=1.0).contains(&anchor.position) {
                            return Err(format!(
                                "二次部材 ID {} の支持アンカー位置が範囲外です",
                                id.0
                            ));
                        }
                        pending.push(anchor.support);
                    }
                }
            }
        }
        Ok(())
    }
}

impl BeamPlacement {
    fn new(
        member: SupportMemberId,
        reference_ends: [[f64; 3]; 2],
        eccentricity: Option<f64>,
        section: &Section,
        reference: [f64; 3],
    ) -> Result<Self, String> {
        let vertical_eccentricity = eccentricity.ok_or("鉛直偏心が未指定です")?;
        if !vertical_eccentricity.is_finite() {
            return Err("鉛直偏心が非有限です".into());
        }
        let actual_ends = actual_beam_axis_ends(reference_ends, vertical_eccentricity)?;
        let axes = local_frame_axes(actual_ends[0], actual_ends[1], reference)?;
        let length_mm = dist(actual_ends[0], actual_ends[1]);
        if !length_mm.is_finite() || length_mm <= 0.0 {
            return Err("材軸長が有限な正値ではありません".into());
        }
        let section_shape = section.shape.clone().ok_or("実断面形状が未指定です")?;
        let geometry = SectionGeometry::of(&section_shape)?;
        if geometry.parts.is_empty() {
            return Err("実断面形状が空です".into());
        }
        Ok(Self {
            member,
            reference_ends,
            vertical_eccentricity,
            actual_ends,
            axes,
            section_shape,
            geometry,
            length_mm,
        })
    }

    /// 指定 XY [mm] を通る鉛直線と有限実形状の交点の下端・上端 Z [mm]。交差なしは None。
    pub fn vertical_range(&self, xy_mm: [f64; 2]) -> Result<Option<[f64; 2]>, String> {
        let origin_z = self.actual_ends[0][2];
        self.line_range([xy_mm[0], xy_mm[1], origin_z], [0.0, 0.0, 1.0])
            .and_then(|range| {
                let result = range.map(|r| r.map(|z| z + origin_z));
                if result.is_some_and(|r| !r.iter().all(|z| z.is_finite())) {
                    return Err("交点Zが非有限です".into());
                }
                Ok(result)
            })
            .map_err(|reason| format!("{}: {reason}", beam_label(self.member)))
    }

    /// 点 [mm] を通り材軸の水平直交方向に沿う実形状の幅 [mm]。交差なしは None。
    pub fn horizontal_width(&self, point_mm: [f64; 3]) -> Result<Option<f64>, String> {
        let x = self.axes[0];
        let norm = x[0].hypot(x[1]);
        if norm == 0.0 {
            return Err(format!(
                "{}: 水平材軸方向が不明です",
                beam_label(self.member)
            ));
        }
        self.line_range(point_mm, [-x[1] / norm, x[0] / norm, 0.0])
            .and_then(|range| {
                let width = range.map(|[lo, hi]| hi - lo);
                if width.is_some_and(|w| !w.is_finite()) {
                    return Err("水平幅が非有限です".into());
                }
                Ok(width)
            })
            .map_err(|reason| format!("{}: {reason}", beam_label(self.member)))
    }

    fn line_range(
        &self,
        origin: [f64; 3],
        direction: [f64; 3],
    ) -> Result<Option<[f64; 2]>, String> {
        if !origin.iter().all(|v| v.is_finite()) {
            return Err(format!("{}: 照会位置が非有限です", beam_label(self.member)));
        }
        let offset = sub(origin, self.actual_ends[0]);
        let p = self.axes.map(|axis| dot(offset, axis));
        let v = self.axes.map(|axis| dot(direction, axis));
        if !p.iter().all(|x| x.is_finite()) {
            return Err(format!(
                "{}: 照会座標を解決できません",
                beam_label(self.member)
            ));
        }
        let mut cap = [f64::NEG_INFINITY, f64::INFINITY];
        if !clip(&mut cap, p[0], v[0], [0.0, self.length_mm])? {
            return Ok(None);
        }
        let mut intervals = Vec::new();
        for (primitive, material) in &self.geometry.parts {
            let mut span = cap;
            match *primitive {
                Primitive::Rectangle { y, z } => {
                    if !clip(&mut span, p[1], v[1], y)? || !clip(&mut span, p[2], v[2], z)? {
                        continue;
                    }
                }
                Primitive::Circle { center, radius } => {
                    let dy = p[1] - center[0];
                    let dz = p[2] - center[1];
                    let a = v[1] * v[1] + v[2] * v[2];
                    let b = dy * v[1] + dz * v[2];
                    if ![dy, dz, a, b].iter().all(|x| x.is_finite()) {
                        return Err(format!(
                            "{}: 断面交点の導出が非有限です",
                            beam_label(self.member)
                        ));
                    }
                    if a == 0.0 {
                        if dy.hypot(dz) > radius {
                            continue;
                        }
                    } else {
                        let perpendicular = dy * v[2] - dz * v[1];
                        let discriminant = a * radius * radius - perpendicular * perpendicular;
                        if !discriminant.is_finite() {
                            return Err(format!(
                                "{}: 断面交点の判別式が非有限です",
                                beam_label(self.member)
                            ));
                        }
                        if discriminant < 0.0 {
                            continue;
                        }
                        span[0] = span[0].max((-b - discriminant.sqrt()) / a);
                        span[1] = span[1].min((-b + discriminant.sqrt()) / a);
                    }
                }
            }
            if span[0] <= span[1] {
                intervals.push((span, material));
            }
        }
        let mut boundaries: Vec<_> = intervals.iter().flat_map(|(span, _)| *span).collect();
        boundaries.sort_by(f64::total_cmp);
        boundaries.dedup();
        let mut result: Option<[f64; 2]> = None;
        for pair in boundaries
            .iter()
            .map(|v| [*v, *v])
            .chain(boundaries.windows(2).map(|p| [p[0], p[1]]))
        {
            let mid = pair[0] + (pair[1] - pair[0]) / 2.0;
            if intervals
                .iter()
                .rev()
                .find(|(span, _)| span[0] <= mid && mid <= span[1])
                .is_some_and(|(_, material)| material.is_some())
            {
                match &mut result {
                    Some(range) => {
                        range[0] = range[0].min(pair[0]);
                        range[1] = range[1].max(pair[1]);
                    }
                    None => result = Some([pair[0], pair[1]]),
                }
            }
        }
        if result.is_some_and(|r| !r.iter().all(|v| v.is_finite())) {
            return Err(format!(
                "{}: 実形状交点が非有限です",
                beam_label(self.member)
            ));
        }
        Ok(result)
    }
}

fn actual_beam_axis_ends(
    reference_ends: [[f64; 3]; 2],
    eccentricity_mm: f64,
) -> Result<[[f64; 3]; 2], String> {
    let mut ends = reference_ends;
    for (side, name) in ["i端", "j端"].iter().enumerate() {
        if !reference_ends[side].iter().all(|v| v.is_finite()) {
            return Err(format!("{name}の基準位置が非有限です"));
        }
        ends[side] = add(reference_ends[side], [0.0, 0.0, eccentricity_mm]);
        if !ends[side].iter().all(|v| v.is_finite()) {
            return Err(format!("{name}の実材軸端が非有限です"));
        }
    }
    Ok(ends)
}

fn beam_label(member: SupportMemberId) -> String {
    match member {
        SupportMemberId::Primary(id) => format!("大梁 ID {}", id.0),
        SupportMemberId::Secondary(id) => format!("小梁 ID {}", id.0),
    }
}

fn clip(span: &mut [f64; 2], offset: f64, slope: f64, bounds: [f64; 2]) -> Result<bool, String> {
    if slope == 0.0 {
        return Ok(bounds[0] <= offset && offset <= bounds[1]);
    }
    let a = (bounds[0] - offset) / slope;
    let b = (bounds[1] - offset) / slope;
    if !a.is_finite() || !b.is_finite() {
        return Err("実形状交点の導出が非有限です".into());
    }
    span[0] = span[0].max(a.min(b));
    span[1] = span[1].min(a.max(b));
    Ok(span[0] <= span[1])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::section_shape::{BeamStirrup, RcBeamRebar, SectionShape};

    fn rebar() -> RcBeamRebar {
        RcBeamRebar {
            main_dia: 22.0,
            top: vec![3],
            bottom: vec![3],
            cover: 40.0,
            stirrup: BeamStirrup {
                dia: 10.0,
                pitch: 150.0,
                legs: 2,
            },
        }
    }

    fn section(depth_mm: f64, src: bool) -> Section {
        let shape = if src {
            SectionShape::SrcBeamRect {
                b: 400.0,
                d: depth_mm,
                rebar: rebar(),
                steel_height: 400.0,
                steel_width: 200.0,
                steel_web_thick: 10.0,
                steel_flange_thick: 20.0,
            }
        } else {
            SectionShape::RcBeamRect {
                b: 400.0,
                d: depth_mm,
                rebar: rebar(),
            }
        };
        let mut section = shape.to_section(SectionId(0), "G1".into());
        section.frame_use = Some(FrameSectionUse::Girder);
        section
    }

    fn placement(
        ends: [[f64; 3]; 2],
        offset_mm: f64,
        section: &Section,
        reference: [f64; 3],
    ) -> BeamPlacement {
        BeamPlacement::new(
            SupportMemberId::Primary(ElemId(12)),
            ends,
            Some(offset_mm),
            section,
            reference,
        )
        .unwrap()
    }

    fn near(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-7, "{actual} != {expected}");
    }

    #[test]
    fn rc_src_horizontal_explicit_positive_negative_zero_and_depth() {
        for src in [false, true] {
            for (offset, axis, bottom, top) in [
                (0.0, -500.0, -800.0, -200.0),
                (100.0, -400.0, -700.0, -100.0),
                (-100.0, -600.0, -900.0, -300.0),
            ] {
                let p = placement(
                    [[0.0, 0.0, -500.0], [4000.0, 0.0, -500.0]],
                    offset,
                    &section(600.0, src),
                    [0.0, 0.0, 1.0],
                );
                assert_eq!(p.actual_ends, [[0.0, 0.0, axis], [4000.0, 0.0, axis]]);
                assert_eq!(
                    p.vertical_range([2000.0, 0.0]).unwrap(),
                    Some([bottom, top])
                );
                near(
                    p.horizontal_width([2000.0, 0.0, axis]).unwrap().unwrap(),
                    400.0,
                );
                let deeper = placement(
                    p.reference_ends,
                    offset,
                    &section(700.0, src),
                    [0.0, 0.0, 1.0],
                );
                assert_eq!(deeper.actual_ends, p.actual_ends);
                assert_eq!(
                    deeper.vertical_range([2000.0, 0.0]).unwrap(),
                    Some([bottom - 50.0, top + 50.0])
                );
            }
        }
    }

    #[test]
    fn sloped_beam_keeps_reference_slope_and_length() {
        let p = placement(
            [[0.0, 0.0, -500.0], [4000.0, 0.0, -400.0]],
            100.0,
            &section(600.0, false),
            [0.0, 0.0, 1.0],
        );
        assert_eq!(p.actual_ends, [[0.0, 0.0, -400.0], [4000.0, 0.0, -300.0]]);
        assert_eq!(
            crate::geom::vec3::midpoint(p.actual_ends[0], p.actual_ends[1])[2],
            -350.0
        );
        near(p.length_mm, 4001.249804748512);
        near(dist(p.reference_ends[0], p.reference_ends[1]), p.length_mm);
    }

    #[test]
    fn thirty_degree_slope_uses_same_xy_intersection_and_finite_end_caps() {
        let p = placement(
            [[0.0, 0.0, 0.0], [3464.1016151377544, 0.0, 2000.0]],
            0.0,
            &section(600.0, false),
            [0.0, 0.0, 1.0],
        );
        let range = p
            .vertical_range([1732.0508075688772, 0.0])
            .unwrap()
            .unwrap();
        near(range[0], 653.5898384862246);
        near(range[1], 1346.4101615137754);
        assert!((range[1] - 1259.8076211353316).abs() > 80.0);
        let start = p.vertical_range([0.0, 0.0]).unwrap().unwrap();
        near(start[0], 0.0);
        near(start[1], 346.4101615137754);
        assert!(p.vertical_range([-200.0, 0.0]).unwrap().is_none());
        let tip = p.vertical_range([-150.0, 0.0]).unwrap().unwrap();
        near(tip[0], 259.8076211353316);
        near(tip[1], 259.8076211353316);
    }

    #[test]
    fn asymmetric_section_and_rotation_use_real_shape_not_half_depth() {
        let s = SectionShape::SteelTee {
            height: 600.0,
            width: 400.0,
            web_thick: 20.0,
            flange_thick: 20.0,
        }
        .to_section(SectionId(0), "T".into());
        let p = placement([[0.0; 3], [4000.0, 0.0, 0.0]], 0.0, &s, [0.0, 0.0, 1.0]);
        let [bottom, top] = p.vertical_range([2000.0, 0.0]).unwrap().unwrap();
        // ウェブ面積11600、フランジ面積8000、下端からの図心は412.4489795918mm。
        near(bottom, -412.44897959183675);
        near(top, 187.55102040816325);
        let rotated = placement(
            p.reference_ends,
            0.0,
            &section(600.0, false),
            [0.0, 1.0, 0.0],
        );
        assert_eq!(
            rotated.vertical_range([2000.0, 0.0]).unwrap(),
            Some([-200.0, 200.0])
        );
        near(
            rotated
                .horizontal_width([2000.0, 0.0, 0.0])
                .unwrap()
                .unwrap(),
            600.0,
        );
    }

    #[test]
    fn round_hollow_shape_tangent_and_overflow_are_not_silently_omitted() {
        let s = SectionShape::SteelPipe {
            outer_dia: 600.0,
            thick: 20.0,
        }
        .to_section(SectionId(0), "P".into());
        let p = placement([[0.0; 3], [4000.0, 0.0, 0.0]], 0.0, &s, [0.0, 0.0, 1.0]);
        assert_eq!(
            p.vertical_range([2000.0, 0.0]).unwrap(),
            Some([-300.0, 300.0])
        );
        assert_eq!(p.vertical_range([2000.0, 300.0]).unwrap(), Some([0.0, 0.0]));
        assert!(p
            .vertical_range([2000.0, 1e300])
            .unwrap_err()
            .contains("大梁 ID 12"));
    }

    #[test]
    fn missing_and_invalid_inputs_report_member_and_reason() {
        let s = section(600.0, false);
        for e in [None, Some(f64::NAN), Some(f64::INFINITY)] {
            assert!(BeamPlacement::new(
                SupportMemberId::Primary(ElemId(12)),
                [[0.0; 3], [4000.0, 0.0, 0.0]],
                e,
                &s,
                [0.0, 0.0, 1.0]
            )
            .is_err());
        }
        let mut missing = s.clone();
        missing.shape = None;
        assert!(BeamPlacement::new(
            SupportMemberId::Primary(ElemId(12)),
            [[0.0; 3], [4000.0, 0.0, 0.0]],
            Some(0.0),
            &missing,
            [0.0, 0.0, 1.0]
        )
        .is_err());
        let mut invalid = s.clone();
        invalid.shape = Some(SectionShape::RcBeamRect {
            b: 400.0,
            d: -600.0,
            rebar: rebar(),
        });
        assert!(BeamPlacement::new(
            SupportMemberId::Primary(ElemId(12)),
            [[0.0; 3], [4000.0, 0.0, 0.0]],
            Some(0.0),
            &invalid,
            [0.0, 0.0, 1.0]
        )
        .is_err());
    }

    #[test]
    fn primary_secondary_placements_follow_nodes_not_sl_and_preserve_supports() {
        let mut model = Model {
            sections: vec![section(600.0, false)],
            ..Default::default()
        };
        for (i, coord) in [[0.0, 0.0, -500.0], [4000.0, 0.0, -400.0]]
            .into_iter()
            .enumerate()
        {
            model.nodes.push(Node {
                id: NodeId(i as u32),
                coord,
                restraint: Default::default(),
                mass: None,
                story: None,
                support_spring: None,
            });
        }
        model.elements.push(ElementData {
            id: ElemId(0),
            kind: ElementKind::Beam,
            nodes: [NodeId(0), NodeId(1)].into_iter().collect(),
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 0.0, 1.0],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            plastic_zone: None,
            spring: None,
            vertical_eccentricity: Some(1000.0),
        });
        let anchors = [0.25, 0.75].map(|position| SecondaryMemberAnchor {
            support: SupportMemberId::Primary(ElemId(0)),
            position,
        });
        model.unassigned_beams.push(SecondaryMember {
            id: SecondaryMemberId(0),
            kind: SecondaryMemberKind::Beam,
            ends: SecondaryMemberEnds::Supported(anchors),
            section: Some(SectionId(0)),
            name: "B1".into(),
            gravity_end_shares: None,
            vertical_eccentricity: Some(100.0),
        });
        let primary = SupportMemberId::Primary(ElemId(0));
        let secondary = SupportMemberId::Secondary(SecondaryMemberId(0));
        let p = model.beam_placement(secondary).unwrap();
        assert_eq!(
            p.reference_ends,
            [[1000.0, 0.0, -475.0], [3000.0, 0.0, -425.0]]
        );
        assert_eq!(
            p.actual_ends,
            [[1000.0, 0.0, -375.0], [3000.0, 0.0, -325.0]]
        );
        assert_eq!(
            model.beam_placement(primary).unwrap().actual_ends,
            [[0.0, 0.0, 500.0], [4000.0, 0.0, 600.0]]
        );
        model
            .stories
            .push(Story::new(StoryId(0), "1F".into(), Some(0.0), Some(-500.0)));
        model.stories[0].sl_offset_mm = Some(-1500.0);
        assert_eq!(
            model.beam_placement(secondary).unwrap().actual_ends,
            p.actual_ends
        );
        model.nodes[1].coord[2] += 100.0;
        assert_eq!(
            model.beam_placement(secondary).unwrap().actual_ends,
            [[1000.0, 0.0, -350.0], [3000.0, 0.0, -250.0]]
        );
        assert_eq!(
            model.unassigned_beams[0].ends,
            SecondaryMemberEnds::Supported(anchors)
        );
        assert_eq!(model.elements[0].nodes.as_slice(), &[NodeId(0), NodeId(1)]);
        let mut encoded = serde_json::to_value(&model).unwrap();
        encoded["elements"][0]
            .as_object_mut()
            .unwrap()
            .remove("vertical_eccentricity");
        encoded["unassigned_beams"][0]
            .as_object_mut()
            .unwrap()
            .remove("vertical_eccentricity");
        let restored: Model = serde_json::from_value(encoded).unwrap();
        assert_eq!(restored.elements[0].vertical_eccentricity, None);
        assert_eq!(restored.unassigned_beams[0].vertical_eccentricity, None);
        assert_eq!(restored.nodes, model.nodes);
        model.unassigned_beams[0].vertical_eccentricity = None;
        let reason = model.beam_placement(secondary).err().unwrap();
        assert!(reason.contains("未指定") && reason.contains("小梁 ID 0"));
        model.unassigned_beams[0].vertical_eccentricity = Some(100.0);
        model.unassigned_beams.push(SecondaryMember {
            id: SecondaryMemberId(1),
            kind: SecondaryMemberKind::Beam,
            ends: SecondaryMemberEnds::Supported([0.25, 0.75].map(|position| {
                SecondaryMemberAnchor {
                    support: secondary,
                    position,
                }
            })),
            section: Some(SectionId(0)),
            name: "B2".into(),
            gravity_end_shares: None,
            vertical_eccentricity: Some(0.0),
        });
        for position in [-0.25, 1.1, f64::INFINITY, f64::NAN] {
            let mut invalid_anchors = anchors;
            invalid_anchors[0].position = position;
            model.unassigned_beams[0].ends = SecondaryMemberEnds::Supported(invalid_anchors);
            for (target, id) in [
                (secondary, 0),
                (SupportMemberId::Secondary(SecondaryMemberId(1)), 1),
            ] {
                let reason = model.beam_placement(target).err().unwrap();
                assert!(
                    reason.contains(&format!("小梁 ID {id}"))
                        && reason.contains("二次部材 ID 0 の支持アンカー位置が範囲外"),
                    "{reason}"
                );
            }
        }
    }
}
