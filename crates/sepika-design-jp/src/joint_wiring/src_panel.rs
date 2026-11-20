//! SRC造柱梁接合部の検討構面・諸元・応力の解決。

use super::ForcesAt;
use crate::rc::joint::JointShape;
use crate::srrc::joint_hoops::resolve_src_short_term_joint_hoops;
use crate::srrc::panel_zone::{
    src_panel_zone_check, SrcPanelBeam, SrcPanelInput, SrcPanelResult, SrcPanelTerm,
};
use crate::{CheckOutcome, LoadTerm};
use sepika_core::adjacency::NodeAdjacency;
use sepika_core::face_distance::{girder_column_faces, GirderColumnFace};
use sepika_core::geom::vec3::{cross, dot, local_frame_axes, norm, unit_from};
use sepika_core::ids::{ElemId, NodeId};
use sepika_core::material_grade::{
    resolve_steel_f, src_h_steel_minimum_thickness_mm, validate_src_h_steel_thickness,
};
use sepika_core::model::{
    src_joint_columns, ElementData, EndCondition, FrameSectionUse, Material, MaterialCategory,
    Model,
};
use sepika_core::panel_zone::{
    member_orientation, panel_joint_planes, MemberOrientation, PanelPlane, PANEL_DIRECTION_TOL,
};
use sepika_core::section_shape::{RcBeamRebar, SectionShape};
use sepika_core::structure_kind::{member_structure_kind, StructureKind};
use sepika_core::units::ConcreteClass;
use sepika_solver::statics::linear::GirderColumnFaceEnds;

const LABEL: &str = "柱梁接合部(SRC)";
const GEOMETRY_TOL_MM: f64 = 1e-6;

#[derive(Clone, Copy)]
pub(crate) enum GirderFaceForces<'a> {
    Analysis(&'a [(ElemId, GirderColumnFaceEnds)]),
    EvaluationPoints(&'a [(ElemId, ForcesAt<'a>)]),
}

pub(super) fn check_src_panel(
    model: &Model,
    adjacency: &NodeAdjacency,
    forces: GirderFaceForces<'_>,
    node: NodeId,
    connected_brace: Option<ElemId>,
    term: LoadTerm,
    out: &mut Vec<(NodeId, String, CheckOutcome)>,
) {
    let connected = adjacency.elements_at(model, node).collect::<Vec<_>>();
    let columns = src_joint_columns(model, adjacency, node).collect::<Vec<_>>();
    let has_src_column = connected.iter().any(|member| {
        model.element_section(member).is_some_and(|section| {
            (section.frame_use == Some(FrameSectionUse::Column)
                && member_structure_kind(model, member) == StructureKind::Src)
                || (section.frame_use.is_none()
                    && matches!(section.shape, Some(SectionShape::SrcColumnRect { .. })))
        })
    });
    if connected.len() < 2 || !has_src_column {
        return;
    }
    let unresolved = connected
        .iter()
        .find(|member| {
            model
                .element_section(member)
                .and_then(|section| section.frame_use)
                .is_none()
        })
        .map(|member| {
            format!(
                "接続部材 ID {} の断面または断面用途が未設定のため接合部を確定できません",
                member.id.0
            )
        });
    let planes = panel_joint_planes(model, node, connected);
    if planes.is_empty() {
        if let Some(reason) = unresolved {
            let label = format!("{LABEL} 検討面未確定");
            out.push((node, label.clone(), CheckOutcome::Skipped { reason }));
            append_transfer_check(out, node, &label);
        }
        return;
    }
    for plane in &planes {
        let label = format!("{LABEL} {}", plane.label());
        let result = match (&unresolved, connected_brace) {
            (Some(reason), _) => Err(reason.clone()),
            (None, Some(brace)) => Err(format!(
                "同じ節点に取り付くブレース ID {} の接合部内の力の伝達経路が未確認です",
                brace.0
            )),
            (None, None) => check_independent_plane(model, plane, &planes)
                .and_then(|()| check_plane(model, adjacency, &columns, forces, node, plane, term)),
        };
        let outcome = match result {
            Ok((shape, result)) => CheckOutcome::Checked(result.into_check_result(shape, term)),
            Err(reason) => CheckOutcome::Skipped {
                reason: format!("接合部 ID {}: {reason}", node.0),
            },
        };
        out.push((node, label.clone(), outcome));
        append_transfer_check(out, node, &label);
    }
}

fn append_transfer_check(out: &mut Vec<(NodeId, String, CheckOutcome)>, node: NodeId, label: &str) {
    out.push((
        node,
        format!("{label} 応力伝達"),
        CheckOutcome::Skipped {
            reason: "20条1項の応力伝達は別途確認が必要です。柱・梁の部分曲げ耐力和が未確定のため(20.3)または(解20.13)の省略条件は判定できません".into(),
        },
    ));
}

fn check_independent_plane(
    model: &Model,
    plane: &PanelPlane,
    planes: &[PanelPlane],
) -> Result<(), String> {
    for other in planes.iter().filter(|other| !std::ptr::eq(*other, plane)) {
        for beam in &other.beams {
            let element = model
                .element(beam.element)
                .ok_or("接続梁を解決できません")?;
            if norm(other.axis) < 0.5
                || beam.outward_axis[2].abs() > PANEL_DIRECTION_TOL
                || dot(plane.axis, other.axis).abs() > PANEL_DIRECTION_TOL
                || element
                    .end_offsets_mm
                    .iter()
                    .any(|offset| offset[0] != 0.0 || offset[1] != 0.0)
            {
                return Err(format!(
                    "梁 ID {} の斜交・勾配・水平偏心または材軸未確定のため検討面を独立に算定できません",
                    element.id.0
                ));
            }
        }
    }
    Ok(())
}

fn check_plane(
    model: &Model,
    adjacency: &NodeAdjacency,
    columns: &[&ElementData],
    forces: GirderFaceForces<'_>,
    node: NodeId,
    plane: &PanelPlane,
    term: LoadTerm,
) -> Result<(JointShape, SrcPanelResult), String> {
    let [column, ..] = columns else {
        return Err("SRC柱が未解決です".into());
    };
    if columns.len() > 2
        || plane.beams.len() > 2
        || (plane.beams.len() == 2
            && !opposed(plane.beams[0].outward_axis, plane.beams[1].outward_axis))
    {
        return Err("同じ側の複数部材または3本以上の部材を含む接合は対象外です".into());
    }
    let normal = [-plane.axis[1], plane.axis[0], 0.0];
    let model_beams = plane
        .beams
        .iter()
        .map(|beam| model.element(beam.element).ok_or("梁を解決できません"))
        .collect::<Result<Vec<_>, _>>()?;
    let [first_beam, ..] = model_beams.as_slice() else {
        return Err("構面内の梁が未解決です".into());
    };

    let col_section = model.element_section(column).ok_or("柱断面が未設定です")?;
    let Some(
        col_shape @ SectionShape::SrcColumnRect {
            b,
            d,
            rebar,
            steel_height,
            steel_flange_thick,
            steel_web_thick,
            ..
        },
    ) = col_section.shape.as_ref()
    else {
        return Err("内蔵H形鉄骨を持つ矩形SRC柱に限ります".into());
    };
    model.ensure_stb_rebar_supported(col_section.id)?;
    col_shape
        .validate_rebar()
        .map_err(|error| error.to_string())?;
    for beam in &model_beams {
        let sec = model.element_section(beam).ok_or("梁断面が未設定です")?;
        model.ensure_stb_rebar_supported(sec.id)?;
    }
    let concrete = model
        .element_material(column)
        .ok_or("柱コンクリート材料が未設定です")?;
    if concrete.category != MaterialCategory::Concrete
        || concrete.concrete_class != ConcreteClass::Normal
    {
        return Err("普通コンクリートの矩形SRC柱に限ります".into());
    }
    let fc = concrete.fc.ok_or("柱コンクリートのFcが未設定です")?;
    let steel = internal_steel(model, column, *steel_web_thick, *steel_flange_thick)?;

    let mut other_ends = Vec::with_capacity(columns.len());
    for candidate in columns {
        let sec = model
            .element_section(candidate)
            .ok_or("柱断面が未設定です")?;
        if sec.shape != col_section.shape
            || sec.steel_material != col_section.steel_material
            || sec.panel_thickness.is_some()
            || model.element_material(candidate) != Some(concrete)
        {
            return Err("上下の異断面・異材または接合部の板厚変更を伴うSRC柱は対象外です".into());
        }
        model.ensure_stb_rebar_supported(sec.id)?;
        let (ends, [axis, depth, _]) = standard_frame(model, candidate, node)?;
        if axis[0].abs() > PANEL_DIRECTION_TOL || axis[1].abs() > PANEL_DIRECTION_TOL {
            return Err("非鉛直柱は対象外です".into());
        }
        if !parallel(depth, plane.axis) {
            return Err("検討面に平行なSRC柱の連続鉄骨ウェブがありません。内蔵Hの弱軸・斜め方向は対象外です".into());
        }
        other_ends.push(candidate.nodes[1 - ends]);
    }

    let z = model.node(node).ok_or("接合部節点が未解決です")?.coord[2];
    if let [a, b] = other_ends.as_slice() {
        let za = model.node(*a).ok_or("柱の反対側節点が未解決です")?.coord[2];
        let zb = model.node(*b).ok_or("柱の反対側節点が未解決です")?.coord[2];
        if (za - z).signum() == (zb - z).signum() {
            return Err("同じ側に複数の柱が付く接合は対象外です".into());
        }
    }

    let (beam, beam_depth) = beam_geometry(model, first_beam)?;
    let mut sum_beam_moments = 0.0;
    for &member in &model_beams {
        let (end, [axis, depth, strong]) = standard_frame(model, member, node)?;
        if axis[2].abs() > PANEL_DIRECTION_TOL
            || !parallel(axis, plane.axis)
            || !parallel(depth, [0.0, 0.0, 1.0])
        {
            return Err("勾配梁・横倒し梁または断面が傾いた梁は対象外です".into());
        }
        if model
            .element_section(member)
            .and_then(|sec| sec.shape.as_ref())
            != model
                .element_section(first_beam)
                .and_then(|sec| sec.shape.as_ref())
        {
            return Err("構面内で異種・異断面の梁が取り付く接合は対象外です".into());
        }
        beam_geometry(model, member)?;
        let faces = girder_column_faces(model, member, adjacency)
            .ok_or("実柱面を評価する大梁に限ります")?;
        let face = faces[end].as_ref().map_err(Clone::clone)?;
        if columns
            .iter()
            .any(|column| !face.columns.contains(&column.id))
        {
            return Err(format!(
                "大梁 ID {} の実柱面が接合部のすべての柱に対応していません",
                member.id.0
            ));
        }
        sum_beam_moments += beam_face_moment(forces, member.id, end, face)?
            * dot(strong, normal)
            * if end == 0 { -1.0 } else { 1.0 };
    }

    let shape = match (columns.len(), plane.beams.len()) {
        (2, 2) => JointShape::Cross,
        (1, 2) => JointShape::Tee,
        (2, 1) => JointShape::Knee,
        _ => JointShape::Corner,
    };
    let h_ratio = story_ratio(model, adjacency, plane.axis, z, &other_ends, beam_depth)?;
    let points = rebar
        .bar_positions(*b, *d)
        .map_err(|error| error.to_string())?;
    let m_cd = group_centroids_distance(points.into_iter().map(|bar| bar.y))?;
    let term = match term {
        LoadTerm::Long => {
            if !steel.young.is_finite()
                || steel.young <= 0.0
                || !concrete.young.is_finite()
                || concrete.young <= 0.0
            {
                return Err("柱の鉄骨・コンクリートのヤング係数は有限の正値が必要です".into());
            }
            SrcPanelTerm::Long {
                n_ratio: steel.young / concrete.young,
            }
        }
        LoadTerm::Short => {
            if matches!(beam, SrcPanelBeam::Rc { .. }) {
                return Err(
                    "RC梁の短期式(20.2)に用いる梁鉄骨フランジ間距離に代わる有効高さと鉄骨ウェブへの応力伝達が未確認です"
                        .into(),
                );
            }
            let hoops = resolve_src_short_term_joint_hoops(model, adjacency, node, plane.axis)?;
            let f = resolve_steel_f(Some(steel), Some(*steel_web_thick))?;
            SrcPanelTerm::Short {
                hoop_ratio: hoops.ratio,
                hoop_allowable_tension: hoops.allowable_tension_n_per_mm2,
                web_allowable_shear: crate::steel::steel_fs(f, LoadTerm::Short),
            }
        }
    };
    src_panel_zone_check(&SrcPanelInput {
        shape,
        beam,
        term,
        fc,
        col_width: *b,
        m_cd,
        j_tw: *steel_web_thick,
        s_cd: *steel_height - *steel_flange_thick,
        h_ratio,
        sum_beam_moments,
    })
    .map(|result| (shape, result))
}

fn internal_steel<'a>(
    model: &'a Model,
    member: &ElementData,
    web_thick_mm: f64,
    flange_thick_mm: f64,
) -> Result<&'a Material, String> {
    let steel = model
        .element_steel_material(member)
        .filter(|steel| steel.category == MaterialCategory::Steel)
        .ok_or_else(|| format!("部材 ID {} の内蔵鉄骨材料が未設定です", member.id.0))?;
    let minimum_mm = src_h_steel_minimum_thickness_mm(steel)?;
    validate_src_h_steel_thickness(&steel.name, web_thick_mm, flange_thick_mm, minimum_mm)?;
    Ok(steel)
}

fn standard_frame(
    model: &Model,
    element: &ElementData,
    node: NodeId,
) -> Result<(usize, [[f64; 3]; 3]), String> {
    let [i, j] = element.nodes.as_slice() else {
        return Err(format!("部材 ID {}: 二節点の線材に限ります", element.id.0));
    };
    let side = usize::from(*j == node);
    if element.end_offsets_mm != [[0.0; 3]; 2] {
        let kind = if member_orientation(model, element) == Some(MemberOrientation::Column) {
            "柱"
        } else {
            "梁"
        };
        return Err(format!(
            "{kind} ID {}: 非0XYZ偏心材は対象外です",
            element.id.0
        ));
    }
    if element.end_cond[side] != EndCondition::Fixed {
        return Err(format!(
            "部材 ID {}: 固定接合以外の柱梁端は対象外です",
            element.id.0
        ));
    }
    if model.member_detail(element.id).is_some() {
        return Err(format!(
            "部材 ID {}: ハンチまたは継手の付帯指定を含む断面は対象外です",
            element.id.0
        ));
    }
    let i = model.node(*i).ok_or("部材始端節点が未設定です")?.coord;
    let j = model.node(*j).ok_or("部材終端節点が未設定です")?.coord;
    let actual = model.primary_frame_axis_ends(element)?.unwrap_or([i, j]);
    if actual != [i, j] {
        return Err(format!(
            "部材 ID {}: 実材軸と節点が異なる部材は対象外です",
            element.id.0
        ));
    }
    Ok((side, local_frame_axes(i, j, element.local_axis.ref_vector)?))
}

fn beam_geometry(model: &Model, beam: &ElementData) -> Result<(SrcPanelBeam, f64), String> {
    let sec = model.element_section(beam).ok_or("梁断面が未設定です")?;
    let shape = sec.shape.as_ref().ok_or("梁の実断面が未設定です")?;
    let material = model.element_material(beam).ok_or("梁材料が未設定です")?;
    shape.validate_rebar().map_err(|error| error.to_string())?;
    let beam = match (shape, material.category) {
        (SectionShape::RcBeamRect { b, d, rebar }, MaterialCategory::Concrete) => (
            SrcPanelBeam::Rc {
                width: *b,
                m_bd: beam_rebar_distance(rebar, *d)?,
            },
            *d,
        ),
        (
            SectionShape::SrcBeamRect {
                b,
                d,
                rebar,
                steel_height,
                steel_flange_thick,
                steel_web_thick,
                ..
            },
            MaterialCategory::Concrete,
        ) => {
            internal_steel(model, beam, *steel_web_thick, *steel_flange_thick)?;
            (
                SrcPanelBeam::Src {
                    width: *b,
                    m_bd: beam_rebar_distance(rebar, *d)?,
                    s_bd: *steel_height - *steel_flange_thick,
                },
                *d,
            )
        }
        (
            SectionShape::SteelH {
                height,
                flange_thick,
                ..
            },
            MaterialCategory::Steel,
        ) => {
            shape
                .validate_surface_radius()
                .map_err(|reason| format!("梁 ID {} のH形鋼断面が不正です。{reason}", beam.id.0))?;
            (
                SrcPanelBeam::Steel {
                    s_bd: *height - *flange_thick,
                },
                *height,
            )
        }
        _ => return Err("梁は配筋済み矩形RC/SRCまたは定型H形鋼に限ります".into()),
    };
    Ok(beam)
}

fn beam_rebar_distance(rebar: &RcBeamRebar, depth_mm: f64) -> Result<f64, String> {
    if !rebar.top.iter().any(|&n| n > 0) || !rebar.bottom.iter().any(|&n| n > 0) {
        return Err("梁の上下両側の主筋群が必要です。主筋がない側の重心を確定できません".into());
    }
    Ok(depth_mm - rebar.top_centroid_from_edge() - rebar.bottom_centroid_from_edge())
}

fn beam_face_moment(
    forces: GirderFaceForces<'_>,
    beam: ElemId,
    end: usize,
    face: &GirderColumnFace,
) -> Result<f64, String> {
    let unavailable = || {
        format!(
            "大梁 ID {} {}端の実柱面曲げが未取得または不正です",
            beam.0,
            ["i", "j"][end]
        )
    };
    let moment = match forces {
        GirderFaceForces::Analysis(rows) => {
            let mut member = rows.iter().filter(|(id, _)| *id == beam);
            let (_, ends) = member.next().ok_or_else(unavailable)?;
            if member.next().is_some() {
                return Err(unavailable());
            }
            let response = ends[end].as_ref().map_err(Clone::clone)?;
            let tolerance_mm = 32.0 * f64::EPSILON * face.length_mm;
            if response.face.columns != face.columns
                || !within_mm(response.face.length_mm, face.length_mm, tolerance_mm)
                || !within_mm(response.face.position_mm, face.position_mm, tolerance_mm)
            {
                return Err(format!(
                    "大梁 ID {} {}端の実柱面が現在の接続・位置に一致しません",
                    beam.0,
                    ["i", "j"][end]
                ));
            }
            response.forces.as_ref().map_err(Clone::clone)?[5]
        }
        GirderFaceForces::EvaluationPoints(rows) => {
            let position = face.position_mm / face.length_mm;
            let mut matched = rows
                .iter()
                .filter(|(id, _)| *id == beam)
                .flat_map(|(_, rows)| rows.iter())
                .filter(|(xi, _)| (*xi - position).abs() <= 32.0 * f64::EPSILON);
            let (_, force) = matched.next().ok_or_else(unavailable)?;
            if matched.next().is_some() {
                return Err(unavailable());
            }
            force[5]
        }
    };
    moment.is_finite().then_some(moment).ok_or_else(unavailable)
}

fn story_ratio(
    model: &Model,
    adjacency: &NodeAdjacency,
    plane_axis: [f64; 3],
    z: f64,
    column_ends: &[NodeId],
    beam_depth: f64,
) -> Result<f64, String> {
    let mut dimensions = None;
    for &end in column_ends {
        let adjacent_z = model.node(end).ok_or("柱の反対側節点が未解決です")?.coord[2];
        let h = (adjacent_z - z).abs();
        let mut adjacent_count = 0;
        for beam in adjacency.elements_at(model, end) {
            match model
                .element_section(beam)
                .and_then(|section| section.frame_use)
            {
                Some(FrameSectionUse::Column | FrameSectionUse::Brace) => continue,
                None => {
                    return Err(format!(
                    "隣階の接続部材 ID {} の断面または断面用途が未設定のため柱内法高さが未確定です",
                    beam.id.0
                ))
                }
                Some(FrameSectionUse::Girder) => {}
            }
            let axis = model
                .primary_frame_axis_ends(beam)
                .map_err(|reason| {
                    format!("隣階の梁 ID {} の実材軸が未確定です。{reason}", beam.id.0)
                })?
                .and_then(|[i, j]| unit_from(i, j))
                .ok_or_else(|| format!("隣階の梁 ID {} の実材軸が未確定です", beam.id.0))?;
            let horizontal = axis[0].hypot(axis[1]);
            if horizontal <= PANEL_DIRECTION_TOL {
                return Err(format!("隣階の梁 ID {} の構面を確定できません", beam.id.0));
            }
            let horizontal_axis = [axis[0] / horizontal, axis[1] / horizontal, 0.0];
            if !parallel(horizontal_axis, plane_axis) {
                continue;
            }
            if axis[2].abs() > PANEL_DIRECTION_TOL {
                return Err(format!(
                    "隣階の梁 ID {} に勾配があり、柱内法高さが未確定です",
                    beam.id.0
                ));
            }
            let (_, [_, depth, _]) = standard_frame(model, beam, end)?;
            let (_, adjacent_depth) = beam_geometry(model, beam)?;
            if !parallel(depth, [0.0, 0.0, 1.0]) || !same_length(beam_depth, adjacent_depth) {
                return Err("隣階の同一構面の梁面が一致せず、柱内法高さが未確定です".into());
            }
            adjacent_count += 1;
        }
        if adjacent_count == 0 {
            return Err("隣階の同一構面に梁がなく、柱内法高さが未確定です".into());
        }
        let clear = h - beam_depth;
        if let Some((previous_h, previous_clear)) = dimensions {
            if !same_length(h, previous_h) || !same_length(clear, previous_clear) {
                return Err("上下層で階高または柱内法高さが異なり、採用値を確定できません".into());
            }
        }
        dimensions = Some((h, clear));
    }
    dimensions
        .map(|(h, clear)| clear / h)
        .ok_or_else(|| "階高と柱内法高さが未確定です".into())
}

fn group_centroids_distance(coordinates: impl Iterator<Item = f64>) -> Result<f64, String> {
    let mut counts = [0, 0];
    let mut sums = [0.0, 0.0];
    for value in coordinates {
        if value.abs() <= GEOMETRY_TOL_MM {
            return Err("接合部中央にある柱主筋を左右の主筋群に割り当てられません".into());
        }
        let side = usize::from(value > 0.0);
        counts[side] += 1;
        sums[side] += value;
    }
    if counts.contains(&0) {
        return Err("柱左右の実主筋位置が未確定です".into());
    }
    Ok(sums[1] / f64::from(counts[1]) - sums[0] / f64::from(counts[0]))
}

fn opposed(a: [f64; 3], b: [f64; 3]) -> bool {
    dot(a, b) < 0.0 && parallel(a, b)
}

fn parallel(a: [f64; 3], b: [f64; 3]) -> bool {
    norm(cross(a, b)) <= PANEL_DIRECTION_TOL && dot(a, b).abs() >= 1.0 - PANEL_DIRECTION_TOL
}

fn same_length(a: f64, b: f64) -> bool {
    within_mm(a, b, GEOMETRY_TOL_MM)
}

fn within_mm(a: f64, b: f64, tolerance_mm: f64) -> bool {
    (a - b).abs() <= tolerance_mm
}
