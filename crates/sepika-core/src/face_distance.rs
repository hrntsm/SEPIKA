//! 柱フェース距離（節点から部材フェースまでの距離）の算定。

use crate::adjacency::NodeAdjacency;
use crate::geom::{element_axis as elem_axis, vec3, ORTHOGONAL_DOT_MAX};
use crate::ids::ElemId;
use crate::model::{
    ElementData, FrameSectionUse, SecondaryMember, SecondaryMemberEnds, SecondaryMemberKind,
    Section, SupportMemberId,
};
use crate::model::{ElementKind, Model};
use crate::section_shape::face_geometry::SectionGeometry;

mod rc_column;
pub use rc_column::{
    aligned_girder, rc_column_girder_faces, RcColumnGirderFace, RcColumnGirderFaces,
};

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GirderColumnFace {
    /// 大梁の実材軸のi端から評価位置までの距離 `[mm]`。
    pub position_mm: f64,
    /// 大梁の実材軸の全長 `[mm]`。
    pub length_mm: f64,
    /// 評価位置を与えた接続柱のID。順序はモデルの要素順。
    pub columns: Vec<ElemId>,
}

/// 大梁の実材軸上の実柱面位置。i/j順。面を決められない端は対象・端・理由付きエラー。
/// 大梁用途の主架構線材以外は None。`adjacency` は同一モデルの `NodeAdjacency::primary_frame`。
pub fn girder_column_faces(
    model: &Model,
    elem: &ElementData,
    adjacency: &NodeAdjacency,
) -> Option<[Result<GirderColumnFace, String>; 2]> {
    if !elem.kind.is_primary_frame_support()
        || !model
            .element_section(elem)
            .is_some_and(|section| section.frame_use == Some(FrameSectionUse::Girder))
    {
        return None;
    }
    let fail = |side: usize, reason: String| {
        format!(
            "大梁 ID {} {}端の実柱面: {reason}",
            elem.id.0,
            ["i", "j"][side]
        )
    };
    let geometry = (|| {
        let ends = model
            .primary_frame_axis_ends(elem)?
            .ok_or("大梁の実材軸が未解決です")?;
        Ok::<_, String>((ends, axis_direction(ends[0], ends[1])?))
    })();
    let mut faces = std::array::from_fn(|side| {
        let resolve = || {
            let (ends, forward) = geometry.as_ref().map_err(Clone::clone)?;
            let direction = if side == 0 {
                *forward
            } else {
                vec3::scale(*forward, -1.0)
            };
            let length_mm = vec3::dist(ends[0], ends[1]);
            let mut connected = adjacency
                .elements_at(model, elem.nodes[side])
                .filter(|col| {
                    col.id != elem.id
                        && model.element_section(col).is_some_and(|section| {
                            section.frame_use == Some(FrameSectionUse::Column)
                        })
                })
                .peekable();
            if connected.peek().is_none() {
                return Err("接続柱がありません".into());
            }
            let mut candidates = connected.filter_map(|col| {
                model
                    .girder_exit_at_column_face_mm(col, ends[side], direction, length_mm)
                    .map(|distance_mm| distance_mm.map(|distance_mm| (col.id, distance_mm)))
                    .transpose()
            });
            let (first, distance_mm) = candidates
                .next()
                .ok_or("梁の実材軸が接続柱の側面を横切りません")??;
            let mut columns = vec![first];
            for other in candidates {
                let (id, other_distance_mm) = other?;
                let tol_mm = 32.0 * f64::EPSILON * distance_mm.max(other_distance_mm).max(1.0);
                if (other_distance_mm - distance_mm).abs() > tol_mm {
                    return Err(format!(
                        "柱 ID {}（梁端から {distance_mm} mm）と柱 ID {}（{other_distance_mm} mm）の側面位置が一致しません",
                        first.0, id.0
                    ));
                }
                columns.push(id);
            }
            if !distance_mm.is_finite() || !(0.0..length_mm).contains(&distance_mm) {
                return Err(format!(
                    "柱 ID {} の側面位置が大梁実材軸上の端部範囲外です",
                    first.0
                ));
            }
            let position_mm = if side == 0 {
                distance_mm
            } else {
                length_mm - distance_mm
            };
            Ok(GirderColumnFace {
                position_mm,
                length_mm,
                columns,
            })
        };
        resolve().map_err(|reason| fail(side, reason))
    });
    if matches!(&faces, [Ok(i), Ok(j)] if i.position_mm >= j.position_mm) {
        let [Ok(i), Ok(j)] = &faces else {
            unreachable!()
        };
        let reason = format!(
            "接続柱の側面間に梁区間がありません（i端 {} mm、j端 {} mm）",
            i.position_mm, j.position_mm
        );
        faces = std::array::from_fn(|side| Err(fail(side, reason.clone())));
    }
    Some(faces)
}

/// 梁の偏心前には一意の柱面があり、偏心後はどの接続柱の側面にも交差しない端の柱ID（i/j順）。
pub fn girder_column_face_misses_due_to_offset(
    model: &Model,
    elem: &ElementData,
    adjacency: &NodeAdjacency,
) -> Option<[Vec<ElemId>; 2]> {
    if elem.end_offsets_mm == [[0.0; 3]; 2] {
        return None;
    }
    let ends = model.primary_frame_axis_ends(elem).ok()??;
    let direction = axis_direction(ends[0], ends[1]).ok()?;
    let length_mm = vec3::dist(ends[0], ends[1]);
    let mut unoffset = elem.clone();
    unoffset.end_offsets_mm = [[0.0; 3]; 2];
    let references = girder_column_faces(model, &unoffset, adjacency)?;
    Some(std::array::from_fn(|side| {
        let Ok(reference) = &references[side] else {
            return Vec::new();
        };
        if elem.end_offsets_mm[side] == [0.0; 3] {
            return Vec::new();
        }
        let inward = if side == 0 {
            direction
        } else {
            vec3::scale(direction, -1.0)
        };
        let columns: Vec<_> = adjacency
            .elements_at(model, elem.nodes[side])
            .filter(|col| {
                col.id != elem.id
                    && model
                        .element_section(col)
                        .is_some_and(|sec| sec.frame_use == Some(FrameSectionUse::Column))
            })
            .collect();
        if columns.len() == reference.columns.len()
            && columns.iter().all(|col| {
                reference.columns.contains(&col.id)
                    && matches!(
                        model.girder_exit_at_column_face_mm(col, ends[side], inward, length_mm),
                        Ok(None)
                    )
            })
        {
            reference.columns.clone()
        } else {
            Vec::new()
        }
    }))
}

fn axis_direction(a: [f64; 3], b: [f64; 3]) -> Result<[f64; 3], String> {
    let delta = vec3::sub(b, a);
    let len = vec3::norm(delta);
    if !len.is_finite() || len <= 1e-12 {
        return Err("材軸長が不正です".into());
    }
    Ok(vec3::scale(delta, 1.0 / len))
}

fn section_frame(axis: [f64; 3], reference: [f64; 3]) -> Result<[[f64; 3]; 2], String> {
    let ([_, common_y, common_z], used_fallback) =
        vec3::local_frame_axes_with_fallback([0.0; 3], axis, reference)
            .map_err(|reason| reason.to_string())?;
    if used_fallback {
        return Err("断面方向を材軸直交面へ投影できません".into());
    }
    Ok([common_y, common_z])
}

fn section_geometry(sec: &Section) -> Result<SectionGeometry, String> {
    let shape = sec.shape.as_ref().ok_or_else(|| {
        format!(
            "断面 {} の実形状が未定義です（せい {} mm・幅 {} mm だけでは外形を解決できません）",
            sec.id.0, sec.depth, sec.width
        )
    })?;
    match shape {
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

/// 大梁自重専用の柱面間区間 `[mm]`。接続柱の距離が複数ある端では最小を採る。
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
            let distance = if column.end_offsets_mm != [[0.0; 3]; 2] {
                let beam_ends = model
                    .primary_frame_axis_ends(elem)?
                    .ok_or_else(|| format!("{label} の実材軸が未解決です"))?;
                let inward = axis_direction(beam_ends[end], beam_ends[1 - end])?;
                model
                    .calculation_beam_placement(SupportMemberId::Primary(column.id))?
                    .connection_face_distance_mm(beam_ends[end], inward)
            } else {
                section_face(
                    sec,
                    axis_direction(ca, cb)?,
                    column.local_axis.ref_vector,
                    vec3::scale(direction, if end == 0 { 1.0 } else { -1.0 }),
                )
            }
            .map_err(|error| format!("{label} の端 {}・柱 {}: {error}", end, column.id.0))?;
            minimum = minimum.min(distance);
        }
        if minimum.is_finite() {
            deductions[end] = minimum;
        }
    }
    let length_mm = model
        .primary_frame_axis_ends(elem)?
        .map_or_else(|| vec3::dist(a, b), |ends| vec3::dist(ends[0], ends[1]));
    checked_interval(&label, length_mm, deductions)
}

/// 二次部材の自重作用区間 `[mm]`。間柱・Detached は芯々区間を保持する。
/// 小梁の断面方向は鉛直を材軸直交面へ投影して決める。
/// 鉛直 Column の最外側面交点が支持柱の有限閉区間外ならエラー。
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
                    if support.kind == SecondaryMemberKind::Post {
                        let angle = model
                            .post_design_inputs
                            .iter()
                            .find(|i| i.member == id)
                            .and_then(|i| i.strong_direction_deg)
                            .filter(|v| v.is_finite())
                            .ok_or_else(|| format!("支持間柱 {} の断面の向きが未設定です", id.0))?
                            .to_radians();
                        [angle.cos(), angle.sin(), 0.0]
                    } else {
                        [0.0, 0.0, 1.0]
                    },
                )
            }
        };
        let section = section.ok_or_else(|| {
            format!(
                "{label} の支持材 {:?} の断面を解決できません",
                anchor.support
            )
        })?;
        let support_axis = axis_direction(sa, sb)?;
        let inward = vec3::scale(direction, if end == 0 { 1.0 } else { -1.0 });
        let column = section.frame_use == Some(FrameSectionUse::Column);
        deductions[end] = (|| {
            if let SupportMemberId::Primary(id) = anchor.support {
                let element = model.element(id).unwrap();
                if column && element.end_offsets_mm != [[0.0; 3]; 2] {
                    model
                        .calculation_beam_placement(anchor.support)?
                        .connection_face_distance_mm(if end == 0 { a } else { b }, inward)
                } else {
                    secondary_support_face(
                        section,
                        reference,
                        support_axis,
                        [sa, sb],
                        if end == 0 { a } else { b },
                        inward,
                        column,
                    )
                }
            } else {
                secondary_support_face(
                    section,
                    reference,
                    support_axis,
                    [sa, sb],
                    if end == 0 { a } else { b },
                    inward,
                    column,
                )
            }
        })()
        .map_err(|error| format!("{label} の端 {end}・支持材 {:?}: {error}", anchor.support))?;
    }
    checked_interval(&label, len, deductions)
        .map_err(|error| format!("{error}（支持端 {:?}）", member.ends))
}

fn secondary_support_face(
    section: &Section,
    reference: [f64; 3],
    support_axis: [f64; 3],
    support_ends: [[f64; 3]; 2],
    origin: [f64; 3],
    inward: [f64; 3],
    column: bool,
) -> Result<f64, String> {
    let [sa, sb] = support_ends;
    if column {
        if sa[0] != sb[0] || sa[1] != sb[1] {
            Err("傾斜柱の支持側面には対応していません".into())
        } else {
            section_face(section, support_axis, reference, inward).and_then(|distance| {
                let height_mm = origin[2] + distance * inward[2];
                if height_mm < sa[2].min(sb[2]) || height_mm > sa[2].max(sb[2]) {
                    Err(format!(
                        "最外側面交点の高さ {height_mm} mm が支持柱の有限区間 [{}, {}] mm の外です",
                        sa[2].min(sb[2]),
                        sa[2].max(sb[2])
                    ))
                } else {
                    Ok(distance)
                }
            })
        }
    } else {
        beam_projected_face(section, support_axis, reference, inward)
    }
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

/// 偏心柱を含む接合の実外形から、対象実部材端のフェース距離 `[mm]` を求める。
/// 関係する部材に非0柱偏心がなければ None。
pub fn eccentric_column_face_distance(
    model: &Model,
    target: &ElementData,
    other: &ElementData,
    node: crate::ids::NodeId,
) -> Result<Option<f64>, String> {
    let eccentric_column = |e: &ElementData| {
        e.end_offsets_mm != [[0.0; 3]; 2]
            && model
                .element_section(e)
                .is_some_and(|s| s.frame_use == Some(FrameSectionUse::Column))
    };
    if !eccentric_column(target) && !eccentric_column(other) {
        return Ok(None);
    }
    let ends = model
        .primary_frame_axis_ends(target)?
        .ok_or_else(|| format!("部材 ID {} の実材軸が未解決です", target.id.0))?;
    let side = usize::from(target.nodes[0] != node);
    let inward = axis_direction(ends[side], ends[1 - side])?;
    model
        .calculation_beam_placement(SupportMemberId::Primary(other.id))?
        .connection_face_distance_mm(ends[side], inward)
        .map(Some)
        .map_err(|reason| {
            format!(
                "部材 ID {}・支持部材 ID {}: {reason}",
                target.id.0, other.id.0
            )
        })
}

/// モデルの全要素について、両端の柱フェース距離 `[i 端, j 端]` `[mm]` を求める。
///
/// 添字は `model.elements` の並びと一致する。Beam 以外の要素と、節点が 2 つ
/// 未満の要素は `[0.0, 0.0]`。
pub fn face_distances(model: &Model) -> Vec<[f64; 2]> {
    try_face_distances(model).unwrap_or_else(|reason| panic!("{reason}"))
}

/// 全線材の実接合面距離 `[mm]`。偏心柱の実形状を解決できない接合は対象付きエラー。
pub fn try_face_distances(model: &Model) -> Result<Vec<[f64; 2]>, String> {
    let adjacency = NodeAdjacency::build(model);
    model
        .elements
        .iter()
        .enumerate()
        .map(|(i, e)| {
            if (!e.kind.is_primary_frame_support()
                || (e.kind != ElementKind::Beam && e.end_offsets_mm == [[0.0; 3]; 2]))
                || e.nodes.len() < 2
            {
                return Ok([0.0, 0.0]);
            }
            let axis = elem_axis(model, e);
            let ni = e.nodes[0];
            let nj = e.nodes[e.nodes.len() - 1];
            let mut faces = [
                face_at(model, ni, axis, i, &adjacency),
                face_at(model, nj, axis, i, &adjacency),
            ];
            for (side, node) in [ni, nj].into_iter().enumerate() {
                let mut exact = 0.0_f64;
                let mut participated = false;
                for &index in adjacency.indices_at(node) {
                    let other = &model.elements[index];
                    if index == i
                        || !other.kind.is_primary_frame_support()
                        || vec3::dot(axis, elem_axis(model, other)).abs() >= ORTHOGONAL_DOT_MAX
                    {
                        continue;
                    }
                    if let Some(distance) = eccentric_column_face_distance(model, e, other, node)? {
                        exact = exact.max(distance);
                        participated = true;
                    } else if let Some(sec) = model.element_section(other) {
                        exact = exact.max(sec.depth / 2.0);
                    }
                }
                if participated {
                    faces[side] = exact;
                }
            }
            Ok(faces)
        })
        .collect()
}

/// 壁実領域に面する支持材の図心から外側フェースまで `[mm]`。断面方向・形状不足はエラー。
pub fn wall_support_face(
    model: &Model,
    support: SupportMemberId,
    direction: [f64; 3],
) -> Result<f64, String> {
    let (a, b) = model
        .support_member_axis(support)
        .ok_or("壁支持材の材軸が未設定です")?;
    let (section, reference) = match support {
        SupportMemberId::Primary(id) => {
            let e = model.element(id).ok_or("壁支持材が存在しません")?;
            if e.end_offsets_mm != [[0.0; 3]; 2]
                && model
                    .element_section(e)
                    .is_some_and(|s| s.frame_use == Some(FrameSectionUse::Column))
            {
                return Err(format!(
                    "壁支持柱 ID {}: 非0XYZ偏心の実壁接合面は未対応です",
                    id.0
                ));
            }
            (
                model
                    .element_section(e)
                    .ok_or("壁支持材の断面が未設定です")?,
                e.local_axis.ref_vector,
            )
        }
        SupportMemberId::Secondary(id) => {
            let e = model
                .secondary_member(id)
                .ok_or("壁二次支持材が存在しません")?;
            let section = e
                .section
                .and_then(|id| model.sections.get(id.index()))
                .ok_or("壁二次支持材の断面が未設定です")?;
            let axis = axis_direction(a, b)?;
            let reference = if e.kind == SecondaryMemberKind::Post {
                if let Some(angle) = model
                    .post_design_inputs
                    .iter()
                    .find(|i| i.member == id)
                    .and_then(|i| i.strong_direction_deg)
                {
                    if !angle.is_finite() {
                        return Err("支持間柱の断面の向きが不正です".into());
                    }
                    let theta = angle.to_radians();
                    [theta.cos(), theta.sin(), 0.0]
                } else {
                    [1.0, 0.0, 0.0]
                }
            } else if axis[2].abs() < 0.99 {
                [0.0, 0.0, 1.0]
            } else {
                [1.0, 0.0, 0.0]
            };
            (section, reference)
        }
    };
    if section.width == 0.0 && section.depth == 0.0 {
        return Ok(0.0);
    }
    section_face(section, axis_direction(a, b)?, reference, direction)
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
            end_offsets_mm: [[0.0, 0.0, 0.0]; 2],
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
        let mut large_reference = model.clone();
        for column in &mut large_reference.elements[1..3] {
            column.local_axis.ref_vector = [1e308, 0.0, 0.0];
        }
        assert_eq!(
            girder_self_weight_interval(&large_reference, &large_reference.elements[0]),
            Ok([300.0, 5700.0])
        );
        let kind = ElementKind::Fiber;
        let mut mixed = model.clone();
        mixed.elements[0].kind = kind;
        mixed.elements[1].kind = kind;
        assert_eq!(
            girder_self_weight_interval(&mixed, &mixed.elements[0]).unwrap(),
            [300.0, 5700.0]
        );
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

    #[test]
    fn 壁支持面は有限な同方向参照の大きさで変わらない() {
        let mut model = Model {
            nodes: vec![node(0, [0.0, 0.0, 0.0]), node(1, [4000.0, 0.0, 0.0])],
            elements: vec![elem(0, ElementKind::Beam, 0, 1, 0)],
            sections: vec![section(0, 600.0)],
            ..Default::default()
        };
        model.sections[0].width = 400.0;
        model.sections[0].shape = Some(crate::section_shape::SectionShape::SteelFlatBar {
            width: 400.0,
            thick: 600.0,
        });
        let support = SupportMemberId::Primary(ElemId(0));
        for reference in [[0.0, 1.0, 0.0], [0.0, 1e308, 0.0]] {
            model.elements[0].local_axis.ref_vector = reference;
            assert_eq!(
                wall_support_face(&model, support, [0.0, 1.0, 0.0]),
                Ok(300.0)
            );
        }
    }

    #[test]
    fn 壁支持面は共通直交基底と同じ断面姿勢を使う() {
        let mut model = Model {
            nodes: vec![node(0, [0.0; 3]), node(1, [1.0, 9.0, 0.0])],
            elements: vec![elem(0, ElementKind::Beam, 0, 1, 0)],
            sections: vec![section(0, 600.0)],
            ..Default::default()
        };
        model.sections[0].width = 400.0;
        model.sections[0].shape = Some(crate::section_shape::SectionShape::SteelFlatBar {
            width: 400.0,
            thick: 600.0,
        });
        let support = SupportMemberId::Primary(ElemId(0));
        let reference = [11043152.607484655, 99388373.4673619, 1e-5];
        let common = vec3::local_frame_axes_with_fallback(
            [0.0; 3],
            axis_direction([0.0; 3], [1.0, 9.0, 0.0]).unwrap(),
            reference,
        )
        .unwrap();
        assert!(!common.1);
        for scaled in [reference, reference.map(|value| value * 2.0)] {
            model.elements[0].local_axis.ref_vector = scaled;
            assert_eq!(
                wall_support_face(&model, support, [0.0, 0.0, 1.0]),
                Ok(300.0)
            );
        }
        model.nodes[1].coord = [-1.0, -9.0, 0.0];
        model.elements[0].local_axis.ref_vector = reference;
        assert_eq!(
            wall_support_face(&model, support, [0.0, 0.0, 1.0]),
            Ok(300.0)
        );
    }

    #[test]
    fn 壁支持面は平行参照の丸めを断面方向として採用しない() {
        let mut model = Model {
            nodes: vec![node(0, [0.0, 0.0, 0.0]), node(1, [1.0, 3.0, 5.0])],
            elements: vec![elem(0, ElementKind::Beam, 0, 1, 0)],
            sections: vec![section(0, 600.0)],
            ..Default::default()
        };
        model.sections[0].width = 400.0;
        model.sections[0].shape = Some(crate::section_shape::SectionShape::SteelFlatBar {
            width: 400.0,
            thick: 600.0,
        });
        for (end, reference) in [
            ([1.0, 3.0, 7.0], [1e6, 3e6, 7e6]),
            ([1.0, 3.0, 7.0], [1e8, 3e8, 7e8]),
            ([1.0, 3.0, 7.0], [1e306, 3e306, 7e306]),
            (
                [1.0, 1.0, 6.0],
                [
                    1.6666666666666666e307,
                    1.6666666666666666e307,
                    9.999999999999998e307,
                ],
            ),
            ([1.0, 3.0, 5.0], [2e307, 6e307, 1e308]),
        ] {
            model.nodes[1].coord = end;
            model.elements[0].local_axis.ref_vector = reference;
            let error =
                wall_support_face(&model, SupportMemberId::Primary(ElemId(0)), [1.0, 0.0, 0.0])
                    .unwrap_err();
            assert!(
                matches!(
                    error.as_str(),
                    "断面方向を材軸直交面へ投影できません"
                        | "断面参照方向から単位直交基底を解決できません"
                ),
                "{error}"
            );
        }
    }

    #[test]
    fn 断面基底は回転と材端反転と微小直交成分を保つ() {
        let d = std::f64::consts::FRAC_1_SQRT_2;
        for (axis, reference, expected_y, expected_z) in [
            (
                [1.0, 0.0, 0.0],
                [0.0, 1e308, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
            ),
            (
                [-1.0, 0.0, 0.0],
                [0.0, 1e308, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, -1.0],
            ),
            (
                [d, d, 0.0],
                [-d * 1e308, d * 1e308, 0.0],
                [-d, d, 0.0],
                [0.0, 0.0, 1.0],
            ),
        ] {
            let [y, z] = section_frame(axis, reference).unwrap();
            for k in 0..3 {
                assert!((y[k] - expected_y[k]).abs() < 1e-12);
                assert!((z[k] - expected_z[k]).abs() < 1e-12);
            }
        }
        let [y, z] = section_frame([1.0, 0.0, 0.0], [1e308, 1.1e-12, 2.3e-12]).unwrap();
        let length = (1.1_f64.powi(2) + 2.3_f64.powi(2)).sqrt();
        assert!((y[1] - 1.1 / length).abs() < 1e-12);
        assert!((y[2] - 2.3 / length).abs() < 1e-12);
        assert!((z[1] + 2.3 / length).abs() < 1e-12);
    }

    #[test]
    fn 断面基底は零平行非有限と解決不能を理由付きで拒否する() {
        for reference in [[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1e-20, 0.0]] {
            assert!(section_frame([1.0, 0.0, 0.0], reference)
                .unwrap_err()
                .contains("投影できません"));
        }
        assert!(section_frame([1.0, 0.0, 0.0], [f64::NAN, 1.0, 0.0])
            .unwrap_err()
            .contains("非有限"));
        let d = std::f64::consts::FRAC_1_SQRT_2;
        assert!(section_frame([d, d, 0.0], [d * 1e308, d * 1e308, 0.0])
            .unwrap_err()
            .contains("投影できません"));
        assert!(section_frame([d, d, 0.0], [1e100, 1e100 + 2e84, 0.0])
            .unwrap_err()
            .contains("単位直交基底"));
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

        model.elements.push(elem(1, ElementKind::Wall, 0, 1, 1));
        assert_eq!(face_distances(&model)[0], [0.0, 0.0]);

        model.elements.push(elem(2, ElementKind::Beam, 0, 1, 1));
        assert_eq!(face_distances(&model)[0], [4999.5, 0.0]);
    }
}
