pub(super) fn deserialize_end_offsets<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<[[f64; 3]; 2], D::Error> {
    let offsets = <[[f64; 3]; 2] as serde::Deserialize>::deserialize(deserializer)?;
    if !offsets.iter().flatten().all(|v| v.is_finite()) {
        return Err(serde::de::Error::custom(
            "XYZ偏心の各成分には有限値が必要です",
        ));
    }
    Ok(offsets)
}

use super::*;
use crate::geom::vec3::{add, dist, dot, local_frame_axes, local_frame_axes_with_fallback, sub};
use crate::section_shape::face_geometry::{Primitive, SectionGeometry};
use crate::section_shape::SectionShape;

/// 梁の有限な実配置。座標・偏心・断面寸法は mm、基底は局所 x/y/z 順。
pub struct BeamPlacement {
    pub member: SupportMemberId,
    pub reference_ends: [[f64; 3]; 2],
    pub end_offsets_mm: [[f64; 3]; 2],
    pub actual_ends: [[f64; 3]; 2],
    /// 局所x/y/zの単位直交基底。二次部材の表示幅反転時だけ左手系となる。
    pub axes: [[f64; 3]; 3],
    pub section_shape: crate::section_shape::SectionShape,
    geometry: SectionGeometry,
    length_mm: f64,
}

impl Model {
    /// 大梁・柱・ブレース・小梁・間柱の実配置を導出する。未指定・不正な偏心、位置、断面は対象と理由付きで失敗する。
    pub fn beam_placement(&self, member: SupportMemberId) -> Result<BeamPlacement, String> {
        self.resolve_beam_placement(member, false)
    }

    /// 力学計算用の配置。小梁・間柱の表示偏心は適用せず、基準端と断面を用いる。
    pub fn calculation_beam_placement(
        &self,
        member: SupportMemberId,
    ) -> Result<BeamPlacement, String> {
        self.resolve_beam_placement(member, true)
    }

    /// 梁実材端からの側面出口距離 `[mm]`。側面を横切らないことが確定する場合は None、採用面を解決できない場合はエラー。
    pub(crate) fn girder_exit_at_column_face_mm(
        &self,
        column: &ElementData,
        origin_mm: [f64; 3],
        inward: [f64; 3],
        girder_length_mm: f64,
    ) -> Result<Option<f64>, String> {
        let fail = |reason: &str| format!("柱 ID {}: {reason}", column.id.0);
        let [i, j] = column.nodes.as_slice() else {
            return Err(fail("二節点の柱が必要です"));
        };
        let a = self
            .node(*i)
            .ok_or_else(|| fail("i端節点がありません"))?
            .coord;
        let b = self
            .node(*j)
            .ok_or_else(|| fail("j端節点がありません"))?
            .coord;
        let reference_ends = [a, b];
        let [a, b] = actual_beam_axis_ends(reference_ends, column.end_offsets_mm)
            .map_err(|reason| fail(&reason))?;
        let length_mm = dist(a, b);
        let axis = sub(b, a).map(|component| component / length_mm);
        let start_mm = dot(sub(origin_mm, a), axis);
        let end_mm = start_mm + girder_length_mm * dot(inward, axis);
        if ![length_mm, start_mm, end_mm]
            .iter()
            .all(|value| value.is_finite())
            || length_mm <= 0.0
        {
            return Err(fail("柱の有限な材軸区間を解決できません"));
        }
        let tol_mm = 32.0 * f64::EPSILON * length_mm.max(girder_length_mm).max(1.0);
        if start_mm.max(end_mm) < -tol_mm || start_mm.min(end_mm) > length_mm + tol_mm {
            return Ok(None);
        }
        let section = self
            .element_section(column)
            .ok_or_else(|| fail("断面がありません"))?;
        let shape = section
            .shape
            .as_ref()
            .ok_or_else(|| fail("実断面形状が未指定です"))?;
        let (axes, fallback) =
            local_frame_axes_with_fallback(a, b, column.local_axis.ref_vector).map_err(fail)?;
        if fallback
            && !matches!(
                shape,
                SectionShape::RcColumnCircle { .. }
                    | SectionShape::SteelRoundBar { .. }
                    | SectionShape::SteelPipe { .. }
                    | SectionShape::CftPipe { .. }
            )
        {
            return Err(fail("断面の向きを解決できません"));
        }
        let p = axes.map(|axis| dot(sub(origin_mm, a), axis));
        let v = axes.map(|axis| dot(inward, axis));
        if ![length_mm, p[0], p[1], p[2], v[0], v[1], v[2]]
            .iter()
            .all(|x| x.is_finite())
            || v[1].hypot(v[2]) <= 32.0 * f64::EPSILON
        {
            return Err(fail("梁材軸と柱側面の交点を解決できません"));
        }
        let resolve = || {
            shape.validate_surface_radius()?;
            let mut cap = [f64::NEG_INFINITY, f64::INFINITY];
            if v[0].abs() > 32.0 * f64::EPSILON {
                clip(&mut cap, p[0], v[0], [0.0, length_mm])?;
            }
            let finite_span = [
                cap[0].max(0.0) - tol_mm,
                cap[1].min(girder_length_mm) + tol_mm,
            ];
            let member = SupportMemberId::Primary(column.id);
            // 外接矩形ではウェブ側や隅角の実外周が分からないため、候補外の証明にだけ使う。
            let intersects_envelope = |depth: [f64; 2], width: f64| {
                let mut span = finite_span;
                let half_width = width / 2.0 + tol_mm;
                Ok::<_, String>(
                    span[0] <= span[1]
                        && clip(
                            &mut span,
                            p[1],
                            v[1],
                            [depth[0] - tol_mm, depth[1] + tol_mm],
                        )?
                        && clip(&mut span, p[2], v[2], [-half_width, half_width])?,
                )
            };
            let intersects_complete_section = |geometry: &SectionGeometry| {
                let misses_with_margin = |margin_mm| {
                    BeamPlacement::material_intervals(
                        geometry,
                        p,
                        v,
                        finite_span,
                        member,
                        margin_mm,
                    )
                    .map(|intervals| intervals.is_empty())
                };
                if !misses_with_margin(-tol_mm)? {
                    Ok(true)
                } else if misses_with_margin(tol_mm)? {
                    Ok(false)
                } else {
                    Err("梁材軸と鋼材の交差が境界付近のため確定できません".to_owned())
                }
            };
            let exits_only_at_cap = |guaranteed: &SectionGeometry, continuous_web| {
                let start = cap[0].max(0.0);
                let exit = cap[1];
                if exit <= start + tol_mm || exit >= girder_length_mm - tol_mm {
                    return Ok(false);
                }
                let with_continuous_web;
                let guaranteed = if let Some(web) = continuous_web {
                    // フランジとウェブの内部継目に、境界公差による隙間を作らない。
                    with_continuous_web = SectionGeometry {
                        parts: guaranteed
                            .parts
                            .iter()
                            .copied()
                            .chain([(web, Some(0))])
                            .collect(),
                    };
                    &with_continuous_web
                } else {
                    guaranteed
                };
                let intervals = BeamPlacement::material_intervals(
                    guaranteed,
                    p,
                    v,
                    finite_span,
                    member,
                    -tol_mm,
                )?;
                Ok::<_, String>(
                    intervals
                        .iter()
                        .any(|[entry, end]| *entry <= finite_span[0] && *end >= finite_span[1]),
                )
            };
            let web_band = |depth, width_mm: f64| {
                Some(Primitive::Rectangle {
                    y: depth,
                    z: [-width_mm / 2.0, width_mm / 2.0],
                })
            };
            let (side_exit, known_region) = match *shape {
                SectionShape::SteelH {
                    height,
                    width,
                    root_r,
                    web_thick,
                    flange_thick,
                } => {
                    let depth = height / 2.0;
                    if !intersects_envelope([-depth, depth], width)? {
                        return Ok(None);
                    }
                    // フィレット未指定時もフランジとウェブの板領域には鋼材がある。
                    let cap_proof = SectionGeometry::h_geometry(
                        height,
                        width,
                        web_thick,
                        flange_thick,
                        root_r.unwrap_or(0.0),
                        true,
                    );
                    if exits_only_at_cap(
                        &cap_proof,
                        web_band([-depth, depth], web_thick.min(width)),
                    )? {
                        return Ok(None);
                    }
                    let known = if root_r.is_some() {
                        SectionGeometry::of(shape)?
                    } else {
                        cap_proof
                    };
                    if root_r.is_some() && !intersects_complete_section(&known)? {
                        return Ok(None);
                    }
                    let face = flange_face_mm(p, v, [-depth, depth], [width; 2]);
                    (face, known)
                }
                SectionShape::SteelBuiltH {
                    lower_width,
                    upper_width,
                    web_thick,
                    ..
                } => {
                    let known = SectionGeometry::of(shape)?;
                    let depth = known.projected_range([1.0, 0.0])?;
                    if !intersects_envelope(depth, lower_width.max(upper_width))?
                        || exits_only_at_cap(
                            &known,
                            web_band(depth, web_thick.min(lower_width).min(upper_width)),
                        )?
                        || !intersects_complete_section(&known)?
                    {
                        return Ok(None);
                    }
                    let face = flange_face_mm(p, v, depth, [lower_width, upper_width]);
                    (face, known)
                }
                SectionShape::SteelBox {
                    height,
                    width,
                    corner_r: None,
                    ..
                }
                | SectionShape::CftBox {
                    height,
                    width,
                    corner_r: None,
                    ..
                } => {
                    let depth = height / 2.0;
                    if !intersects_envelope([-depth, depth], width)? {
                        return Ok(None);
                    }
                    let guaranteed_outer = SectionGeometry::rounded_outer_envelope(
                        height,
                        width,
                        height.min(width) / 2.0,
                    );
                    if exits_only_at_cap(&guaranteed_outer, None)? {
                        return Ok(None);
                    }
                    let face = flat_face_with_unknown_radius_mm(p, v, [height / 2.0, width / 2.0]);
                    (face, guaranteed_outer)
                }
                SectionShape::RcColumnRect { .. }
                | SectionShape::SrcColumnRect { .. }
                | SectionShape::RcColumnCircle { .. }
                | SectionShape::SteelRoundBar { .. }
                | SectionShape::SteelFlatBar { .. }
                | SectionShape::SteelPipe { .. }
                | SectionShape::CftPipe { .. }
                | SectionShape::SteelBox { .. }
                | SectionShape::CftBox { .. } => {
                    let placement = BeamPlacement::new(
                        SupportMemberId::Primary(column.id),
                        reference_ends,
                        column.end_offsets_mm,
                        section,
                        column.local_axis.ref_vector,
                    )?;
                    return placement.closed_column_side_exit_mm(
                        p,
                        v,
                        cap,
                        girder_length_mm,
                        tol_mm,
                    );
                }
                _ => return Err("接合詳細がなく、柱の採用側面を一意に決められません".into()),
            };
            let intervals = BeamPlacement::material_intervals(
                &known_region,
                p,
                v,
                [f64::NEG_INFINITY, f64::INFINITY],
                member,
                0.0,
            )?;
            let mut finite_intervals = intervals
                .into_iter()
                .filter(|[entry, exit]| *entry <= finite_span[1] && *exit >= finite_span[0]);
            let interval = finite_intervals.next();
            if finite_intervals.next().is_some() {
                return Err(
                    "有限な柱の既知の鋼材区間が連続せず、採用面を決めるには接合詳細が必要です"
                        .into(),
                );
            }
            let side_exit = side_exit?;
            let [entry, exit] =
                interval.ok_or("有限な柱の端面と梁材軸が交わる位置の外周を確定できません")?;
            if (exit - side_exit).abs() > tol_mm {
                return Err("採用面までの既知の柱断面区間を解決できません".into());
            }
            let position = finite_column_side_exit_mm(
                [entry, side_exit],
                cap,
                girder_length_mm,
                tol_mm,
                false,
            )?
            .ok_or("柱の端面だけを通過することを確定できず、接合詳細が必要です")?;
            Ok(Some(position))
        };
        resolve().map_err(|reason| fail(&reason))
    }

    fn resolve_beam_placement(
        &self,
        member: SupportMemberId,
        calculation: bool,
    ) -> Result<BeamPlacement, String> {
        let resolve = || {
            let (ends, eccentricity, section, reference) = match member {
                SupportMemberId::Primary(id) => {
                    let e = self.element(id).ok_or("部材がありません")?;
                    if !(e.kind.is_primary_frame_support()
                        || matches!(e.kind, ElementKind::Brace { .. }))
                        || e.nodes.len() != 2
                    {
                        return Err("大梁の二節点線材ではありません".to_string());
                    }
                    let section = self
                        .section(e.section.ok_or("断面が未指定です")?)
                        .ok_or("断面がありません")?;
                    if !matches!(
                        section.frame_use,
                        Some(
                            FrameSectionUse::Girder
                                | FrameSectionUse::Column
                                | FrameSectionUse::Brace
                        )
                    ) {
                        return Err("断面用途が大梁・柱・ブレースではありません".to_string());
                    }
                    let a = self.node(e.nodes[0]).ok_or("i端節点がありません")?.coord;
                    let b = self.node(e.nodes[1]).ok_or("j端節点がありません")?.coord;
                    ([a, b], e.end_offsets_mm, section, e.local_axis.ref_vector)
                }
                SupportMemberId::Secondary(id) => {
                    let sm = self.secondary_member(id).ok_or("小梁がありません")?;
                    if sm.is_detached() {
                        return Err("支持アンカーを持つ二次部材が必要です".to_string());
                    }
                    self.validate_beam_reference_supports(member)?;
                    let (a, b) = self
                        .secondary_member_end_points(sm)
                        .ok_or("支持アンカーの基準両端位置を解決できません")?;
                    let display_ends = actual_beam_axis_ends([a, b], sm.end_offsets_mm)?;
                    let display_length = dist(display_ends[0], display_ends[1]);
                    if !display_length.is_finite() || display_length <= 0.0 {
                        return Err("表示実材軸長が有限な正値ではありません".to_string());
                    }
                    let section = self
                        .section(sm.section.ok_or("断面が未指定です")?)
                        .ok_or("断面がありません")?;
                    (
                        [a, b],
                        if calculation {
                            [[0.0; 3]; 2]
                        } else {
                            sm.end_offsets_mm
                        },
                        section,
                        if let Some(axis) = sm.display_local_axis.filter(|_| !calculation) {
                            axis.ref_vector
                        } else if sm.kind == SecondaryMemberKind::Post {
                            let angle = self
                                .post_design_inputs
                                .iter()
                                .find(|input| input.member == id)
                                .and_then(|input| input.strong_direction_deg)
                                .unwrap_or(0.0)
                                .to_radians();
                            [angle.cos(), angle.sin(), 0.0]
                        } else {
                            [0.0, 0.0, 1.0]
                        },
                    )
                }
            };
            let mut placement = BeamPlacement::new(member, ends, eccentricity, section, reference)?;
            if !calculation {
                if let SupportMemberId::Secondary(id) = member {
                    if self
                        .secondary_member(id)
                        .and_then(|sm| sm.display_local_axis)
                        .is_some_and(|axis| axis.reverse_width)
                    {
                        placement.axes[2] = placement.axes[2].map(|value| -value);
                    }
                }
            }
            Ok(placement)
        };
        resolve().map_err(|reason| format!("{} の実配置: {reason}", beam_label(member)))
    }

    /// 大梁・柱・ブレースの実材軸端 `[mm]` を返し、局所基底も検証する。
    /// 端位置の扱いは [`Self::primary_frame_axis_end_positions`] と同じ。局所基底が不正なら理由付きエラー。
    pub fn primary_frame_axis_ends(
        &self,
        data: &ElementData,
    ) -> Result<Option<[[f64; 3]; 2]>, String> {
        let ends = self.primary_frame_axis_end_positions(data)?;
        if let Some([i, j]) = ends {
            local_frame_axes(i, j, data.local_axis.ref_vector).map_err(|reason| {
                format!("部材 ID {} i/j端の実材軸・局所軸: {reason}", data.id.0)
            })?;
        }
        Ok(ends)
    }

    /// 大梁・柱・ブレースの実材軸端の位置 `[mm]` を返す。局所基底の検証は呼出側で行う。
    /// 節点位置をそのまま使う無偏心柱・ブレース、および対象外の部材は `None`。
    /// 端位置や偏心の未指定・不正は理由付きエラー。
    pub fn primary_frame_axis_end_positions(
        &self,
        data: &ElementData,
    ) -> Result<Option<[[f64; 3]; 2]>, String> {
        if let Some(side) = data
            .end_offsets_mm
            .iter()
            .position(|r| r.iter().any(|v| !v.is_finite()))
        {
            return Err(format!(
                "部材 ID {} {}のXYZ偏心が非有限です",
                data.id.0,
                ["i端", "j端"][side]
            ));
        }
        let brace = matches!(data.kind, ElementKind::Brace { .. });
        if !data.kind.is_primary_frame_support() && !brace {
            if data.end_offsets_mm != [[0.0; 3]; 2] {
                return Err(format!(
                    "部材 ID {}: この種別の非0偏心は未対応です",
                    data.id.0
                ));
            }
            return Ok(None);
        }
        let fail = |end: &str, item: &str, reason: &str| {
            format!("部材 ID {} {end}の{item}: {reason}", data.id.0)
        };
        if brace && data.end_offsets_mm == [[0.0; 3]; 2] {
            return Ok(None);
        }
        let section = self
            .element_section(data)
            .ok_or_else(|| fail("i/j端", "断面", "未指定または参照先がありません"))?;
        if !brace
            && !matches!(
                section.frame_use,
                Some(FrameSectionUse::Girder | FrameSectionUse::Column)
            )
        {
            if data.end_offsets_mm != [[0.0; 3]; 2] {
                return Err(fail(
                    "i/j端",
                    "XYZ偏心",
                    "大梁・柱以外の非0偏心は未対応です",
                ));
            }
            return Ok(None);
        }
        if !brace
            && section.frame_use == Some(FrameSectionUse::Column)
            && data.end_offsets_mm == [[0.0; 3]; 2]
        {
            return Ok(None);
        }
        if !brace
            && section.frame_use == Some(FrameSectionUse::Column)
            && data.end_offsets_mm != [[0.0; 3]; 2]
        {
            for wall in self.elements.iter().filter(|e| e.kind == ElementKind::Wall) {
                if data.nodes.iter().all(|n| wall.nodes.contains(n)) {
                    return Err(format!("柱 ID {}・壁要素 ID {}: 非0XYZ偏心を持つ側柱と壁の共通断面・接続は未対応です", data.id.0, wall.id.0));
                }
            }
            for plate in &self.wall_plates {
                if plate
                    .boundary_nodes(self)
                    .is_some_and(|nodes| data.nodes.iter().all(|n| nodes.contains(n)))
                {
                    return Err(format!(
                        "柱 ID {}・壁版 ID {}: 非0XYZ偏心を持つ側柱の壁面・接合面は未対応です",
                        data.id.0, plate.id.0
                    ));
                }
            }
        }
        if data.nodes.len() != 2 {
            return Err(fail("i/j端", "節点", "二節点が必要です"));
        }
        let eccentricity = data.end_offsets_mm;
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

fn flange_face_mm(
    p: [f64; 3],
    v: [f64; 3],
    depth: [f64; 2],
    flange_width: [f64; 2],
) -> Result<f64, String> {
    let side = usize::from(v[1] > 0.0);
    let distance_mm = (depth[side] - p[1]) / v[1];
    let width_at_face_mm = (p[2] + distance_mm * v[2]).abs();
    let tol_mm = 32.0 * f64::EPSILON * flange_width[side].max(1.0);
    if v[1].abs() > 32.0 * f64::EPSILON
        && distance_mm.is_finite()
        && width_at_face_mm < flange_width[side] / 2.0 - tol_mm
    {
        Ok(distance_mm)
    } else {
        Err(
            "H形柱の外フランジ広面の内部に交わらず、接合詳細から採用面を決める必要があります"
                .into(),
        )
    }
}

fn flat_face_with_unknown_radius_mm(
    p: [f64; 3],
    v: [f64; 3],
    half_mm: [f64; 2],
) -> Result<f64, String> {
    let max_radius_mm = half_mm[0].min(half_mm[1]);
    let flats_mm = half_mm.map(|half| half - max_radius_mm);
    for side in 0..2 {
        let other = 1 - side;
        let component = v[side + 1];
        if component == 0.0 {
            continue;
        }
        let distance_mm = (component.signum() * half_mm[side] - p[side + 1]) / component;
        // 最大の角Rでも残る平面上の点だけは、半径にかかわらず同じ外周に属する。
        if distance_mm.is_finite()
            && (p[other + 1] + distance_mm * v[other + 1]).abs()
                <= flats_mm[other] + 32.0 * f64::EPSILON * half_mm[other]
        {
            return Ok(distance_mm);
        }
    }
    Err("角Rが未知で、梁材軸と柱外周の交点を一意に決められません".into())
}

fn finite_column_side_exit_mm(
    [side_entry, side_exit]: [f64; 2],
    cap: [f64; 2],
    girder_length_mm: f64,
    tol_mm: f64,
    tangent: bool,
) -> Result<Option<f64>, String> {
    let entry = side_entry.max(cap[0]);
    let exit = side_exit.min(cap[1]);
    let overlap_mm = exit.min(girder_length_mm) - entry.max(0.0);
    if overlap_mm < -tol_mm {
        return Ok(None);
    }
    if tangent
        || overlap_mm <= tol_mm
        || (side_exit - cap[1]).abs() <= tol_mm
        || (side_entry >= -tol_mm && (side_entry - cap[0]).abs() <= tol_mm)
    {
        return Err("梁材軸が柱の境界だけに接するか、側面と端面の交点を分けられません".into());
    }
    if side_entry > tol_mm && side_entry > cap[0] {
        return Err(
            "実材端と離れた位置で梁材軸が柱側面へ入る可能性があり、接合詳細が必要です".into(),
        );
    }
    if cap[1] < side_exit {
        return Ok(None);
    }
    Ok(Some(side_exit))
}

impl BeamPlacement {
    fn closed_column_side_exit_mm(
        &self,
        p: [f64; 3],
        v: [f64; 3],
        cap: [f64; 2],
        girder_length_mm: f64,
        tol_mm: f64,
    ) -> Result<Option<f64>, String> {
        let norm = v[1].hypot(v[2]);
        let normal = [v[2] / norm, -v[1] / norm];
        let offset = p[1] * normal[0] + p[2] * normal[1];
        let [lo, hi] = self.geometry.projected_range(normal)?;
        if offset < lo - tol_mm || offset > hi + tol_mm {
            return Ok(None);
        }
        let intervals = Self::material_intervals(
            &self.geometry,
            p,
            v,
            [f64::NEG_INFINITY, f64::INFINITY],
            self.member,
            0.0,
        )?;
        let (first, last) = intervals
            .first()
            .zip(intervals.last())
            .ok_or("梁材軸が柱外周に接するかを判定できません")?;
        // 鋼管の中空部の境界を柱の側面に採ることはできない。
        finite_column_side_exit_mm(
            [first[0], last[1]],
            cap,
            girder_length_mm,
            tol_mm,
            offset <= lo + tol_mm || offset >= hi - tol_mm,
        )
    }

    /// 水平面内の材軸直交方向への実断面外側投影幅 `[mm]`。
    pub fn plan_width_mm(&self) -> Result<f64, String> {
        let x = self.axes[0];
        let norm = x[0].hypot(x[1]);
        if !norm.is_finite() || norm <= 0.0 {
            return Err(format!(
                "{}: 水平材軸方向が不明です",
                beam_label(self.member)
            ));
        }
        let direction = [-x[1] / norm, x[0] / norm, 0.0];
        let range = self
            .geometry
            .projected_range([dot(self.axes[1], direction), dot(self.axes[2], direction)])?;
        Ok(range[1] - range[0])
    }

    /// 材軸のXYを通る鉛直線と梁実上面の交点が線形になる材軸位置 `[mm]`。
    pub fn top_surface_breaks_mm(&self) -> Result<Vec<f64>, String> {
        let mut cuts = vec![0.0, self.length_mm];
        for offset in self.vertical_material_offsets_mm()?.into_iter().flatten() {
            for end in [0.0, self.length_mm] {
                let position = end - offset * self.axes[0][2];
                if !position.is_finite() {
                    return Err(format!(
                        "{}: 上端切替位置が非有限です",
                        beam_label(self.member)
                    ));
                }
                if position > 0.0 && position < self.length_mm {
                    cuts.push(position);
                }
            }
        }
        cuts.sort_by(f64::total_cmp);
        cuts.dedup();
        Ok(cuts)
    }

    /// 切替位置を内部に含まない材軸区間 `[mm]` の中心線上実上端Zの両端極限 `[mm]`。
    /// 上端の不連続点では区間内からの極限を返し、区間不正・交点未解決はエラー。
    pub fn top_surface_on_interval_mm(&self, interval: [f64; 2]) -> Result<[f64; 2], String> {
        let [a, b] = interval;
        let fail = |reason: &str| format!("{}: {reason}", beam_label(self.member));
        if !interval.iter().all(|s| s.is_finite()) || a < 0.0 || b > self.length_mm || b <= a {
            return Err(fail("上端照会区間が不正です"));
        }
        if self
            .top_surface_breaks_mm()?
            .iter()
            .any(|s| *s > a && *s < b)
        {
            return Err(fail("上端照会区間が材料・端面の切替位置を跨いでいます"));
        }
        let middle = a + (b - a) / 2.0;
        if middle <= a || middle >= b {
            return Err(fail("上端照会区間の内部点を解決できません"));
        }
        let slope = self.axes[0][2];
        let mut cap = [f64::NEG_INFINITY, f64::INFINITY];
        if !clip(&mut cap, middle, slope, [0.0, self.length_mm])? {
            return Err(fail("上端照会区間に有限梁の交点がありません"));
        }
        let selected = self
            .vertical_material_offsets_mm()?
            .into_iter()
            .filter(|span| span[0].max(cap[0]) <= span[1].min(cap[1]))
            .max_by(|left, right| left[1].min(cap[1]).total_cmp(&right[1].min(cap[1])))
            .ok_or_else(|| fail("中心線上の鉛直線に実材料の交点がありません"))?;
        let top = if selected[1] <= cap[1] {
            interval.map(|s| self.actual_ends[0][2] + slope * s + selected[1])
        } else {
            let end = if slope > 0.0 { self.length_mm } else { 0.0 };
            interval.map(|s| self.actual_ends[0][2] + slope * s + (end - s) / slope)
        };
        if !top.iter().all(|z| z.is_finite()) {
            return Err(fail("実上端Zが非有限です"));
        }
        Ok(top)
    }

    fn vertical_material_offsets_mm(&self) -> Result<Vec<[f64; 2]>, String> {
        self.line_material_intervals(self.actual_ends[0], [0.0, 0.0, 1.0], false)
    }

    fn new(
        member: SupportMemberId,
        reference_ends: [[f64; 3]; 2],
        eccentricity: [[f64; 3]; 2],
        section: &Section,
        reference: [f64; 3],
    ) -> Result<Self, String> {
        let end_offsets_mm = eccentricity;
        let actual_ends = actual_beam_axis_ends(reference_ends, end_offsets_mm)?;
        let axes = local_frame_axes(actual_ends[0], actual_ends[1], reference)?;
        let length_mm = dist(actual_ends[0], actual_ends[1]);
        if !length_mm.is_finite() || length_mm <= 0.0 {
            return Err("材軸長が有限な正値ではありません".into());
        }
        let section_shape = section.shape.clone().ok_or("実断面形状が未指定です")?;
        let geometry = SectionGeometry::of(&section_shape)?;
        Ok(Self {
            member,
            reference_ends,
            end_offsets_mm,
            actual_ends,
            axes,
            section_shape,
            geometry,
            length_mm,
        })
    }

    /// 指定 XY `[mm]` を通る鉛直線と有限実形状の交点の下端・上端 Z `[mm]`。交差なしは None。
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

    /// 点 `[mm]` を通り材軸の水平直交方向に沿う実形状の幅 `[mm]`。交差なしは None。
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

    /// 実部材端から単位方向に進むときの支持実外形の出口距離 `[mm]`。交差なしは0、途中だけの交差は未対応として失敗する。
    pub fn connection_face_distance_mm(
        &self,
        origin: [f64; 3],
        direction: [f64; 3],
    ) -> Result<f64, String> {
        self.connection_face_mm(origin, direction, 1e-6)
            .map(|distance| distance.unwrap_or(0.0))
    }

    /// 原点に実部材端の座標 `[mm]`、方向に単位ベクトルを渡し、接触する支持実外形の出口距離 `[mm]` を返す。
    /// 端点だけの接触は距離0、非交差はNone、正方向の途中だけで交わる場合はエラー。
    pub fn connection_face_at_contact_mm(
        &self,
        origin: [f64; 3],
        direction: [f64; 3],
    ) -> Result<Option<f64>, String> {
        self.connection_face_mm(origin, direction, 0.0)
    }

    fn connection_face_mm(
        &self,
        origin: [f64; 3],
        direction: [f64; 3],
        entry_tolerance_mm: f64,
    ) -> Result<Option<f64>, String> {
        let Some([entry, exit]) = self.line_range(origin, direction)? else {
            return Ok(None);
        };
        if exit < 0.0 {
            return Ok(None);
        }
        if entry > entry_tolerance_mm {
            return Err(format!(
                "{}: 実材端と離れた位置で支持実形状と交差する接合は未対応です",
                beam_label(self.member)
            ));
        }
        Ok(Some(exit.max(0.0)))
    }

    fn line_range(
        &self,
        origin: [f64; 3],
        direction: [f64; 3],
    ) -> Result<Option<[f64; 2]>, String> {
        let intervals = self.line_material_intervals(origin, direction, true)?;
        Ok(intervals
            .first()
            .zip(intervals.last())
            .map(|(first, last)| [first[0], last[1]]))
    }

    fn line_material_intervals(
        &self,
        origin: [f64; 3],
        direction: [f64; 3],
        finite_ends: bool,
    ) -> Result<Vec<[f64; 2]>, String> {
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
        if finite_ends && !clip(&mut cap, p[0], v[0], [0.0, self.length_mm])? {
            return Ok(Vec::new());
        }
        Self::material_intervals(&self.geometry, p, v, cap, self.member, 0.0)
    }

    fn material_intervals(
        geometry: &SectionGeometry,
        p: [f64; 3],
        v: [f64; 3],
        cap: [f64; 2],
        member: SupportMemberId,
        boundary_margin_mm: f64,
    ) -> Result<Vec<[f64; 2]>, String> {
        let mut intervals = Vec::new();
        for (primitive, material) in &geometry.parts {
            let mut span = cap;
            // 境界付近を確定扱いしないため、非交差の証明では材料を広げ、鋼材内部の通過の証明では狭める。
            let margin_mm = if material.is_some() {
                boundary_margin_mm
            } else {
                -boundary_margin_mm
            };
            match *primitive {
                Primitive::Rectangle { y, z } => {
                    let expanded = |[lo, hi]: [f64; 2]| [lo - margin_mm, hi + margin_mm];
                    let y = expanded(y);
                    let z = expanded(z);
                    if y[1] < y[0]
                        || z[1] < z[0]
                        || !clip(&mut span, p[1], v[1], y)?
                        || !clip(&mut span, p[2], v[2], z)?
                    {
                        continue;
                    }
                }
                Primitive::Circle { center, radius } => {
                    let radius = radius + margin_mm;
                    if radius < 0.0 {
                        continue;
                    }
                    let dy = p[1] - center[0];
                    let dz = p[2] - center[1];
                    let a = v[1] * v[1] + v[2] * v[2];
                    let b = dy * v[1] + dz * v[2];
                    if ![dy, dz, a, b].iter().all(|x| x.is_finite()) {
                        return Err(format!(
                            "{}: 断面交点の導出が非有限です",
                            beam_label(member)
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
                                beam_label(member)
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
        let mut result: Vec<[f64; 2]> = Vec::new();
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
                result.push(pair);
            }
        }
        if !result.iter().flatten().all(|v| v.is_finite()) {
            return Err(format!("{}: 実形状交点が非有限です", beam_label(member)));
        }
        result.sort_by(|left, right| {
            left[0]
                .total_cmp(&right[0])
                .then(left[1].total_cmp(&right[1]))
        });
        let mut merged: Vec<[f64; 2]> = Vec::new();
        for span in result {
            if let Some(previous) = merged.last_mut().filter(|previous| span[0] <= previous[1]) {
                previous[1] = previous[1].max(span[1]);
            } else {
                merged.push(span);
            }
        }
        Ok(merged)
    }
}

fn actual_beam_axis_ends(
    reference_ends: [[f64; 3]; 2],
    eccentricity_mm: [[f64; 3]; 2],
) -> Result<[[f64; 3]; 2], String> {
    let mut ends = reference_ends;
    for (side, name) in ["i端", "j端"].iter().enumerate() {
        if !reference_ends[side].iter().all(|v| v.is_finite()) {
            return Err(format!("{name}の基準位置が非有限です"));
        }
        if !eccentricity_mm[side].iter().all(|v| v.is_finite()) {
            return Err(format!("{name}のXYZ偏心が非有限です"));
        }
        ends[side] = add(reference_ends[side], eccentricity_mm[side]);
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
            main_dia: crate::section_shape::RebarSize::DiameterMm(22.0),
            top: vec![3],
            bottom: vec![3],
            cover: 40.0,
            stirrup: BeamStirrup {
                dia: crate::section_shape::RebarSize::DiameterMm(10.0),
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
                steel_manufacture: Default::default(),
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
            [[0.0, 0.0, offset_mm]; 2],
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
        for e in [[[f64::NAN; 3]; 2], [[f64::INFINITY; 3]; 2]] {
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
            [[0.0; 3]; 2],
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
            [[0.0; 3]; 2],
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
            end_offsets_mm: [[0.0, 0.0, 1000.0]; 2],
        });
        let anchors = [0.25, 0.75].map(|position| SecondaryMemberAnchor {
            support: SupportMemberId::Primary(ElemId(0)),
            position,
        });
        model.unassigned_beams.push(SecondaryMember {
            display_local_axis: None,
            id: SecondaryMemberId(0),
            kind: SecondaryMemberKind::Beam,
            ends: SecondaryMemberEnds::Supported(anchors),
            section: Some(SectionId(0)),
            name: "B1".into(),
            gravity_end_shares: None,
            end_offsets_mm: [[0.0, 0.0, 100.0]; 2],
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
            .remove("end_offsets_mm");
        encoded["unassigned_beams"][0]
            .as_object_mut()
            .unwrap()
            .remove("end_offsets_mm");
        assert!(serde_json::from_value::<Model>(encoded).is_err());
        let mut null = serde_json::to_value(&model).unwrap();
        null["elements"][0]["end_offsets_mm"] = serde_json::Value::Null;
        assert!(serde_json::from_value::<Model>(null).is_err());
        model.unassigned_beams[0].end_offsets_mm = [[0.0, 0.0, 100.0]; 2];
        model.unassigned_beams.push(SecondaryMember {
            display_local_axis: None,
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
            end_offsets_mm: [[0.0, 0.0, 0.0]; 2],
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
    #[test]
    fn xyz_offsets_preserve_common_translation_and_use_each_physical_end() {
        let ends = [[0.0, 0.0, 0.0], [6000.0, 0.0, 0.0]];
        let section = section(600.0, false);
        let make = |offsets| {
            BeamPlacement::new(
                SupportMemberId::Primary(ElemId(0)),
                ends,
                offsets,
                &section,
                [0.0, 0.0, 1.0],
            )
            .unwrap()
        };
        let original = make([[0.0; 3]; 2]);
        let shifted = make([[100.0, 200.0, 300.0]; 2]);
        assert_eq!(
            shifted.actual_ends,
            [[100.0, 200.0, 300.0], [6100.0, 200.0, 300.0]]
        );
        assert_eq!(shifted.reference_ends, ends);
        assert_eq!(shifted.axes, original.axes);
        assert_eq!(shifted.length_mm, 6000.0);
        let inclined = make([[0.0; 3], [0.0, 0.0, 300.0]]);
        assert_eq!(inclined.actual_ends[1], [6000.0, 0.0, 300.0]);
        assert!((inclined.length_mm - 6007.49531835024).abs() < 1e-10);
        let reversed = BeamPlacement::new(
            SupportMemberId::Primary(ElemId(0)),
            [ends[1], ends[0]],
            [[0.0, 0.0, 300.0], [0.0; 3]],
            &section,
            [0.0, 0.0, 1.0],
        )
        .unwrap();
        assert_eq!(
            reversed.actual_ends,
            [inclined.actual_ends[1], inclined.actual_ends[0]]
        );
        assert_eq!(reversed.length_mm, inclined.length_mm);
        assert_eq!(reversed.axes[0], inclined.axes[0].map(|v| -v));
    }
    #[test]
    fn column_xyz_placement_uses_independent_coordinates_and_reversed_local_axis() {
        let mut model = Model {
            sections: vec![section(600.0, false)],
            ..Default::default()
        };
        model.sections[0].frame_use = Some(FrameSectionUse::Column);
        for (id, coord) in [[0.0, 0.0, 0.0], [0.0, 0.0, 3000.0]]
            .into_iter()
            .enumerate()
        {
            model.nodes.push(Node {
                id: NodeId(id as u32),
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
            nodes: vec![NodeId(0), NodeId(1)].into(),
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [1.0, 0.0, 0.0],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            plastic_zone: None,
            spring: None,
            end_offsets_mm: [[0.0; 3]; 2],
        });
        for (offsets, expected, length, axis) in [
            (
                [[0.0; 3]; 2],
                [[0.0, 0.0, 0.0], [0.0, 0.0, 3000.0]],
                3000.0,
                [0.0, 0.0, 1.0],
            ),
            (
                [[100.0, 200.0, 300.0]; 2],
                [[100.0, 200.0, 300.0], [100.0, 200.0, 3300.0]],
                3000.0,
                [0.0, 0.0, 1.0],
            ),
            (
                [[0.0; 3], [400.0, 0.0, 0.0]],
                [[0.0, 0.0, 0.0], [400.0, 0.0, 3000.0]],
                3026.549190084311,
                [0.13216372009101796, 0.0, 0.9912279006826347],
            ),
        ] {
            for reverse in [false, true] {
                model.elements[0].nodes = if reverse {
                    vec![NodeId(1), NodeId(0)].into()
                } else {
                    vec![NodeId(0), NodeId(1)].into()
                };
                model.elements[0].end_offsets_mm = if reverse {
                    [offsets[1], offsets[0]]
                } else {
                    offsets
                };
                let expected = if reverse {
                    [expected[1], expected[0]]
                } else {
                    expected
                };
                let actual = model
                    .primary_frame_axis_ends(&model.elements[0])
                    .unwrap()
                    .unwrap_or_else(|| {
                        model.elements[0]
                            .nodes
                            .iter()
                            .map(|id| model.node(*id).unwrap().coord)
                            .collect::<Vec<_>>()
                            .try_into()
                            .unwrap()
                    });
                assert_eq!(actual, expected);
                let placement = model
                    .beam_placement(SupportMemberId::Primary(ElemId(0)))
                    .unwrap();
                assert_eq!(placement.actual_ends, expected);
                near(placement.length_mm, length);
                for (actual, expected) in placement.axes[0].into_iter().zip(axis) {
                    near(actual, if reverse { -expected } else { expected });
                }
            }
        }
    }
    #[test]
    fn eccentric_column_support_face_uses_actual_shape_and_rejects_remote_crossings() {
        let section = section(600.0, false);
        let placement = BeamPlacement::new(
            SupportMemberId::Primary(ElemId(7)),
            [[0.0, 0.0, 0.0], [0.0, 0.0, 3000.0]],
            [[50.0, 0.0, 0.0]; 2],
            &section,
            [1.0, 0.0, 0.0],
        )
        .unwrap();
        assert_eq!(
            placement.axes,
            [[0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
        );
        assert!(matches!(
            section.shape,
            Some(SectionShape::RcBeamRect {
                b: 400.0,
                d: 600.0,
                ..
            })
        ));
        near(
            placement
                .connection_face_distance_mm([0.0, 0.0, 1500.0], [1.0, 0.0, 0.0])
                .unwrap(),
            350.0,
        );
        near(
            placement
                .connection_face_distance_mm([0.0, 0.0, 1500.0], [-1.0, 0.0, 0.0])
                .unwrap(),
            250.0,
        );
        assert_eq!(
            placement
                .connection_face_distance_mm([0.0, 1000.0, 1500.0], [1.0, 0.0, 0.0])
                .unwrap(),
            0.0
        );
        let reason = placement
            .connection_face_distance_mm([-500.0, 0.0, 1500.0], [1.0, 0.0, 0.0])
            .unwrap_err();
        assert!(
            reason.contains("ID 7") && reason.contains("離れた位置"),
            "{reason}"
        );
    }
}
