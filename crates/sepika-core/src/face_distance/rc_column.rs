use crate::adjacency::NodeAdjacency;
use crate::geom::vec3::{
    cross, dist, dot, local_frame_axes, local_frame_axes_and_fallback_reference, norm, scale, sub,
    unit,
};
use crate::ids::{ElemId, NodeId};
use crate::model::{BeamPlacement, ElementData, FrameSectionUse, Model, SupportMemberId};
use crate::section_shape::SectionShape;
use crate::structure_kind::{structure_kind_of, StructureKind};

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RcColumnGirderFace {
    /// 実材軸i端から検定面までの距離 [mm]。
    pub position_mm: f64,
    /// 柱の実材軸長 [mm]。
    pub length_mm: f64,
    /// 断面座標と同じ [mm] の集中荷重を、面上とみなせる場合はtrue。
    pub position_exact: bool,
}

pub struct RcColumnGirderFaces {
    /// i/j順。大梁の確認を要しない端はNone、未確定な端は理由付きエラー。
    pub ends: [Option<Result<RcColumnGirderFace, String>>; 2],
    /// 確定した両面が一致または逆転するときはfalse。
    pub has_positive_span: bool,
}

/// 柱軸に直交する外面を持つ矩形大梁の実配置と、その面に垂直な局所軸の添字。
/// `inward` は柱区間へ向く単位ベクトル。
pub fn aligned_girder(
    model: &Model,
    girder: ElemId,
    inward: [f64; 3],
) -> Result<(BeamPlacement, usize), String> {
    let placement = model.calculation_beam_placement(SupportMemberId::Primary(girder))?;
    let normal = girder_normal(&placement.section_shape, placement.axes, inward, girder)?;
    Ok((placement, normal))
}

fn girder_normal(
    shape: &SectionShape,
    axes: [[f64; 3]; 3],
    inward: [f64; 3],
    girder: ElemId,
) -> Result<usize, String> {
    if matches!(
        shape,
        SectionShape::RcBeamRect { .. } | SectionShape::SrcBeamRect { .. }
    ) {
        if let Some(normal) = (1..=2).find(|&i| norm(cross(axes[i], inward)) <= 32.0 * f64::EPSILON)
        {
            return Ok(normal);
        }
    }
    Err(format!(
        "大梁 ID {} と柱の接合境界を一つの断面位置で表せません",
        girder.0
    ))
}

/// 通常RC柱検定の接続大梁で決まる実材端面。剛域距離や材中央での切り詰めは用いない。
/// 矩形・円形RC柱以外はNone。`adjacency` は同一モデルの主架構線材の隣接関係。
pub fn rc_column_girder_faces(
    model: &Model,
    elem: &ElementData,
    adjacency: &NodeAdjacency,
) -> Option<RcColumnGirderFaces> {
    let sec = model.element_section(elem)?;
    let mat = model.element_material(elem)?;
    let shape = sec.shape.as_ref()?;
    if !elem.kind.is_primary_frame_support()
        || sec.frame_use != Some(FrameSectionUse::Column)
        || structure_kind_of(Some(sec), Some(mat.category)) != StructureKind::Rc
        || !matches!(
            shape,
            SectionShape::RcColumnRect { .. } | SectionShape::RcColumnCircle { .. }
        )
    {
        return None;
    }
    let unknown = |reason: &str| {
        Some(RcColumnGirderFaces {
            ends: std::array::from_fn(|_| Some(Err(reason.into()))),
            has_positive_span: false,
        })
    };
    let &[i, j] = elem.nodes.as_slice() else {
        return unknown("柱の両実材端節点がありません");
    };
    let axes_at_ends = (|| {
        let reference_ends = [
            model.node(i).ok_or("柱のi端節点がありません")?.coord,
            model.node(j).ok_or("柱のj端節点がありません")?.coord,
        ];
        let [a, b] = model
            .primary_frame_axis_ends(elem)?
            .unwrap_or(reference_ends);
        let length_mm = dist(a, b);
        let span_mm = axis_span_mm(reference_ends, elem.end_offsets_mm);
        if norm(sub(span_mm, sub(b, a))) > 32.0 * f64::EPSILON * length_mm.max(1.0) {
            return Err("実配置で求めた柱の材軸と解析材軸の一致を確認できません".into());
        }
        let axes = local_frame_axes([0.0; 3], span_mm, elem.local_axis.ref_vector)?;
        let zero_normal_components =
            std::array::from_fn(|k| same_axis_coordinate(reference_ends, elem.end_offsets_mm, k));
        let exact_axis = cardinal_axis(axes[0]).filter(|&axis| {
            (0..3)
                .filter(|&k| k != axis)
                .all(|k| zero_normal_components[k])
        });
        let exact_unoffset_span = (elem.end_offsets_mm == [[0.0; 3]; 2]
            && (0..3)
                .all(|k| sum_with_residual(reference_ends[1][k], -reference_ends[0][k]).1 == 0.0))
        .then_some(span_mm);
        let declared_direction = exact_unoffset_span.or_else(|| {
            exact_axis.map(|axis| std::array::from_fn(|k| if k == axis { 1.0 } else { 0.0 }))
        });
        let length_exact = exact_axis.is_some_and(|axis| {
            let [origin, far] = reference_ends.map(|p| p[axis]);
            let [offset_i, offset_j] = elem.end_offsets_mm.map(|p| p[axis]);
            let (origin, origin_error) = sum_with_residual(origin, offset_i);
            let (far, far_error) = sum_with_residual(far, offset_j);
            let (_, difference_error) = sum_with_residual(far, -origin);
            far_error == origin_error && difference_error == 0.0
        }) || exact_unoffset_span
            .is_some_and(|span| exact_span_length(span, length_mm));
        Ok::<_, String>((
            axes,
            length_mm,
            exact_axis,
            length_exact,
            zero_normal_components,
            declared_direction,
        ))
    })();
    let nodes = [i, j];
    let mut faces = std::array::from_fn(|side| {
        let joint = adjacency.elements_at(model, nodes[side]).filter(|other| {
            other.id != elem.id
                && !model
                    .element_section(other)
                    .is_some_and(|section| section.frame_use == Some(FrameSectionUse::Column))
        });
        let mut face_at_joint: Option<FaceAtJoint> = None;
        for other in joint {
            let resolve = || {
                let (axes, _, column_axis, _, zero_normal_components, declared_direction) =
                    axes_at_ends.as_ref().map_err(Clone::clone)?;
                if model
                    .element_section(other)
                    .and_then(|section| section.frame_use)
                    != Some(FrameSectionUse::Girder)
                {
                    return Err(format!(
                        "接続部材 ID {} の断面・用途が未確認です",
                        other.id.0
                    ));
                }
                if [
                    elem.rigid_zone.panel_offset_i,
                    elem.rigid_zone.panel_offset_j,
                ][side]
                    != 0.0
                {
                    return Err("仕口パネルがある端の大梁面と柱の接合境界が未確認です".into());
                }
                let inward = scale(axes[0], if side == 0 { 1.0 } else { -1.0 });
                let offset_mm = elem.end_offsets_mm[side];
                check_continuation(model, elem, adjacency, nodes[side], offset_mm, inward)?;
                let beam = model.calculation_beam_placement(SupportMemberId::Primary(other.id))?;
                if column_axis.is_some_and(|axis| {
                    !same_axis_coordinate(beam.reference_ends, beam.end_offsets_mm, axis)
                }) {
                    return Err(format!(
                        "大梁 ID {} の材軸が柱軸に直交しないため接合境界を一つの断面位置で表せません",
                        other.id.0
                    ));
                }
                let beam_span_mm = axis_span_mm(beam.reference_ends, beam.end_offsets_mm);
                let (mut beam_axes, fallback_reference) = local_frame_axes_and_fallback_reference(
                    [0.0; 3],
                    beam_span_mm,
                    other.local_axis.ref_vector,
                )?;
                let reference = fallback_reference.unwrap_or(other.local_axis.ref_vector);
                let normal = girder_normal(&beam.section_shape, beam_axes, inward, other.id)?;
                let beam_side = usize::from(other.nodes.first() != Some(&nodes[side]));
                if model
                    .member_detail(other.id)
                    .and_then(|detail| {
                        if beam_side == 0 {
                            detail.haunch_i
                        } else {
                            detail.haunch_j
                        }
                    })
                    .is_some_and(|haunch| {
                        haunch.depth_increase != 0.0 || haunch.width_increase != 0.0
                    })
                {
                    return Err(format!(
                        "大梁 ID {} の接合側にハンチがあり、実外形を確認できません",
                        other.id.0
                    ));
                }
                let beam_axis = cardinal_axis(beam_axes[0]).filter(|&axis| {
                    (0..3)
                        .filter(|&k| k != axis)
                        .all(|k| same_axis_coordinate(beam.reference_ends, beam.end_offsets_mm, k))
                });
                let oblique = match (beam_axis, declared_direction) {
                    (Some(axis), Some(direction)) => {
                        declared_normal_is_oblique(axis, normal, reference, *direction)
                    }
                    _ => column_axis.is_some_and(|axis| normal == 2 && reference[axis] != 0.0),
                };
                if oblique {
                    return Err(format!(
                        "大梁 ID {} の断面の外面が柱軸に直交しないため接合境界を一つの断面位置で表せません",
                        other.id.0
                    ));
                }
                let exact_axis =
                    column_axis.filter(|&axis| cardinal_axis(beam_axes[normal]) == Some(axis));
                if beam_side == 1 {
                    beam_axes[0] = scale(beam_axes[0], -1.0);
                }
                let face = contact_distance(
                    shape,
                    *axes,
                    inward,
                    offset_mm,
                    JointGirder {
                        beam: &beam,
                        axes: beam_axes,
                        length_mm: norm(beam_span_mm),
                        normal,
                        offset_mm: beam.end_offsets_mm[beam_side],
                        exact_axis,
                        zero_normal_components: *zero_normal_components,
                        normal_input: beam_axis.map(|axis| NormalInput {
                            beam_axis: axis,
                            reference,
                            normal,
                        }),
                    },
                )?;
                if let Some(other) = face_at_joint {
                    other.confirm_same_plane(face)?;
                }
                Ok(face)
            };
            match resolve() {
                Ok(face) => face_at_joint = Some(face),
                Err(reason) => return Some(Err(reason)),
            }
        }
        face_at_joint.map(|face| {
            let (_, length_mm, _, length_exact, _, _) =
                *axes_at_ends.as_ref().expect("確認済みの柱材軸");
            let (position_mm, position_error) = if side == 0 {
                (face.distance_mm, 0.0)
            } else {
                sum_with_residual(length_mm, -face.distance_mm)
            };
            Ok(RcColumnGirderFace {
                position_mm,
                length_mm,
                position_exact: face.distance_exact
                    && position_error == 0.0
                    && (side == 0 || length_exact),
            })
        })
    });
    let [i, j] = faces
        .each_ref()
        .map(|end| end.as_ref().and_then(|cut| cut.as_ref().ok()));
    let has_positive_span = i.is_none_or(|cut| cut.position_mm < cut.length_mm)
        && j.is_none_or(|cut| cut.position_mm > 0.0)
        && !matches!((i, j), (Some(i), Some(j)) if i.position_mm >= j.position_mm);
    if !has_positive_span {
        faces =
            std::array::from_fn(|_| Some(Err("両大梁面の間に正の長さの柱区間がありません".into())));
    }
    Some(RcColumnGirderFaces {
        ends: faces,
        has_positive_span,
    })
}

fn check_continuation(
    model: &Model,
    elem: &ElementData,
    adjacency: &NodeAdjacency,
    node: NodeId,
    offset_mm: [f64; 3],
    inward: [f64; 3],
) -> Result<(), String> {
    for other in adjacency.elements_at(model, node).filter(|other| {
        other.id != elem.id
            && model
                .element_section(other)
                .and_then(|section| section.frame_use)
                == Some(FrameSectionUse::Column)
    }) {
        let &[a, b] = other.nodes.as_slice() else {
            return Err("接続柱の両実材端節点がありません".into());
        };
        model.primary_frame_axis_ends(other)?;
        let reference_ends =
            [a, b].map(|id| model.node(id).map_or([f64::NAN; 3], |node| node.coord));
        let side = usize::from(a != node);
        let span_mm = axis_span_mm(reference_ends, other.end_offsets_mm);
        let direction = unit(scale(span_mm, if side == 0 { 1.0 } else { -1.0 }));
        if other.end_offsets_mm[side] != offset_mm
            || direction.is_none_or(|dir| {
                norm(cross(dir, inward)) > 32.0 * f64::EPSILON || dot(dir, inward) >= 0.0
            })
        {
            return Err("柱の材軸が連続しない接合です".into());
        }
    }
    Ok(())
}

struct JointGirder<'a> {
    beam: &'a BeamPlacement,
    axes: [[f64; 3]; 3],
    length_mm: f64,
    normal: usize,
    offset_mm: [f64; 3],
    exact_axis: Option<usize>,
    zero_normal_components: [bool; 3],
    normal_input: Option<NormalInput>,
}

#[derive(Clone, Copy, PartialEq)]
struct NormalInput {
    beam_axis: usize,
    reference: [f64; 3],
    normal: usize,
}

#[derive(Clone, Copy, PartialEq)]
struct ProjectedPlaneInput {
    normal: NormalInput,
    offset_mm: [f64; 3],
    half_mm: f64,
}

#[derive(Clone, Copy)]
struct FaceAtJoint {
    distance_mm: f64,
    distance_exact: bool,
    exact_plane_mm: Option<(f64, f64)>,
    projected_input: Option<ProjectedPlaneInput>,
    roundoff_mm: f64,
}

impl FaceAtJoint {
    fn confirm_same_plane(self, other: Self) -> Result<(), String> {
        let different = "接続大梁によって材端フェースの位置が異なります";
        match (self.exact_plane_mm, other.exact_plane_mm) {
            (Some(a), Some(b)) if a == b => Ok(()),
            (Some(_), Some(_)) => Err(different.into()),
            (None, None)
                if self.projected_input.is_some()
                    && self.projected_input == other.projected_input =>
            {
                Ok(())
            }
            _ if (self.distance_mm - other.distance_mm).abs()
                > self.roundoff_mm + other.roundoff_mm =>
            {
                Err(different.into())
            }
            _ => Err(
                "接続大梁の材端フェースが同じ位置にあることを丸め誤差の範囲で確認できません".into(),
            ),
        }
    }
}

fn contact_distance(
    column: &SectionShape,
    column_axes: [[f64; 3]; 3],
    inward: [f64; 3],
    column_offset_mm: [f64; 3],
    girder: JointGirder<'_>,
) -> Result<FaceAtJoint, String> {
    let (b, d) = match girder.beam.section_shape {
        SectionShape::RcBeamRect { b, d, .. } | SectionShape::SrcBeamRect { b, d, .. } => (b, d),
        _ => unreachable!(),
    };
    let beam_half = [girder.length_mm / 2.0, d / 2.0, b / 2.0];
    let transverse = 3 - girder.normal;
    // 共通節点の絶対座標を先に加えると、接触を左右する微小な偏心が消える。
    let delta = sub(girder.offset_mm, column_offset_mm);
    let axes = [girder.axes[0], girder.axes[transverse]];
    let half = [beam_half[0], beam_half[transverse]];
    let offset = [dot(delta, axes[0]) + half[0], dot(delta, axes[1])];
    let valid = |x: f64| x.is_finite() && x > 0.0;
    let (margin_mm, column_size_mm) = match *column {
        SectionShape::RcColumnCircle { d, .. } => {
            let distance_mm = (offset[0].abs() - half[0])
                .max(0.0)
                .hypot((offset[1].abs() - half[1]).max(0.0));
            (d / 2.0 - distance_mm, d)
        }
        SectionShape::RcColumnRect { b, d, .. } => {
            let cosines = axes.map(|axis| [dot(axis, column_axes[1]), dot(axis, column_axes[2])]);
            let column_half = [d / 2.0, b / 2.0];
            let margins = (0..2)
                .map(|k| {
                    let [c, s] = cosines[k];
                    half[k] + c.abs() * column_half[0] + s.abs() * column_half[1] - offset[k].abs()
                })
                .chain((0..2).map(|k| {
                    let [c, s] = [cosines[0][k], cosines[1][k]];
                    column_half[k] + c.abs() * half[0] + s.abs() * half[1]
                        - (offset[0] * c + offset[1] * s).abs()
                }));
            (
                margins.fold(f64::INFINITY, f64::min),
                if valid(b) && valid(d) {
                    b.max(d)
                } else {
                    f64::NAN
                },
            )
        }
        _ => unreachable!(),
    };
    let SupportMemberId::Primary(id) = girder.beam.member else {
        unreachable!()
    };
    if !beam_half.into_iter().all(valid) || !valid(column_size_mm) || !margin_mm.is_finite() {
        return Err(format!(
            "大梁 ID {} の実外形と柱の実材端が正の面積で接触しません",
            id.0
        ));
    }
    let scale_mm = beam_half
        .into_iter()
        .chain(delta.map(f64::abs))
        .fold(column_size_mm.max(1.0), f64::max);
    let roundoff_mm = 32.0 * f64::EPSILON * scale_mm;
    let axial_half_mm = beam_half[girder.normal];
    let normal_coordinates = |offset_mm: [f64; 3]| {
        std::array::from_fn(|k| {
            if girder.zero_normal_components[k] {
                0.0
            } else {
                offset_mm[k]
            }
        })
    };
    let (entry_mm, distance_mm, distance_exact, exact_plane_mm) =
        if let Some(axis) = girder.exact_axis {
            let center_mm = girder.offset_mm[axis] * inward[axis].signum();
            let end_mm = column_offset_mm[axis] * inward[axis].signum();
            let entry = sum_with_residual(center_mm, -axial_half_mm);
            let exit = sum_with_residual(center_mm, axial_half_mm);
            let (distance_mm, distance_exact) = plane_offset_mm(exit, end_mm);
            (
                plane_offset_mm(entry, end_mm).0,
                distance_mm,
                distance_exact,
                Some(exit),
            )
        } else {
            let center_mm = dot(normal_coordinates(delta), inward);
            (
                center_mm - axial_half_mm,
                center_mm + axial_half_mm,
                normal_coordinates(girder.offset_mm) == normal_coordinates(column_offset_mm),
                None,
            )
        };
    if exact_plane_mm.is_none() && entry_mm.abs().min(distance_mm.abs()) <= roundoff_mm {
        return Err(format!(
            "大梁 ID {} と柱の実材端が柱軸方向で接触することを丸め誤差の範囲で確認できません",
            id.0
        ));
    }
    if !entry_mm.is_finite() || !distance_mm.is_finite() || entry_mm > 0.0 || distance_mm < 0.0 {
        return Err(format!(
            "大梁 ID {} の実外形と柱の実材端が正の面積で接触しません",
            id.0
        ));
    }
    if margin_mm <= roundoff_mm {
        return Err(if margin_mm >= -roundoff_mm {
            format!(
                "大梁 ID {} と柱の実材端が正の面積で接触することを丸め誤差の範囲で確認できません",
                id.0
            )
        } else {
            format!(
                "大梁 ID {} の実外形と柱の実材端が正の面積で接触しません",
                id.0
            )
        });
    }
    Ok(FaceAtJoint {
        distance_mm,
        distance_exact,
        exact_plane_mm,
        projected_input: girder.normal_input.map(|normal| ProjectedPlaneInput {
            normal,
            offset_mm: normal_coordinates(girder.offset_mm),
            half_mm: axial_half_mm,
        }),
        roundoff_mm,
    })
}

fn cardinal_axis(vector: [f64; 3]) -> Option<usize> {
    (0..3).find(|&axis| {
        vector[axis] != 0.0 && (0..3).all(|other| other == axis || vector[other] == 0.0)
    })
}

fn plane_offset_mm((plane_mm, residual_mm): (f64, f64), end_mm: f64) -> (f64, bool) {
    // 柱端を先に引くと、梁の面が柱端に近いときに離隔が丸めで消える。
    let (distance, subtraction_error) = sum_with_residual(plane_mm, -end_mm);
    let (correction, correction_error) = sum_with_residual(residual_mm, subtraction_error);
    let (rounded, remaining_error) = sum_with_residual(distance, correction);
    (rounded, correction_error == 0.0 && remaining_error == 0.0)
}

fn axis_span_mm(reference_mm: [[f64; 3]; 2], offset_mm: [[f64; 3]; 2]) -> [f64; 3] {
    std::array::from_fn(|k| {
        let (reference, ref_residual) = sum_with_residual(reference_mm[1][k], -reference_mm[0][k]);
        let (offset, offset_residual) = sum_with_residual(offset_mm[1][k], -offset_mm[0][k]);
        let (span, span_residual) = sum_with_residual(reference, offset);
        span + (span_residual + (ref_residual + offset_residual))
    })
}

fn declared_normal_is_oblique(
    beam_axis: usize,
    normal: usize,
    reference: [f64; 3],
    column: [f64; 3],
) -> bool {
    let u = (beam_axis + 1) % 3;
    let v = (beam_axis + 2) % 3;
    let [n_u, n_v] = if normal == 1 {
        [reference[u], reference[v]]
    } else {
        [-reference[v], reference[u]]
    };
    // 正規化後の照合だけでは、入力から分かる斜交まで許容差に隠れる。
    let p = column[u] * n_v;
    let q = column[v] * n_u;
    column[beam_axis] != 0.0
        || p.is_finite()
            && q.is_finite()
            && (p != q || column[u].mul_add(n_v, -p) != column[v].mul_add(n_u, -q))
}

fn exact_span_length(span: [f64; 3], length_mm: f64) -> bool {
    let mut squares = 0.0;
    for span_mm in span {
        let square = span_mm * span_mm;
        let (sum, sum_error) = sum_with_residual(squares, square);
        if span_mm.mul_add(span_mm, -square) != 0.0 || sum_error != 0.0 {
            return false;
        }
        squares = sum;
    }
    length_mm * length_mm == squares && length_mm.mul_add(length_mm, -squares) == 0.0
}

fn same_axis_coordinate(
    reference_mm: [[f64; 3]; 2],
    offset_mm: [[f64; 3]; 2],
    axis: usize,
) -> bool {
    let [a, b] = [0, 1].map(|end| sum_with_residual(reference_mm[end][axis], offset_mm[end][axis]));
    a == b
}

fn sum_with_residual(a: f64, b: f64) -> (f64, f64) {
    let sum = a + b;
    let b_in_sum = sum - a;
    (sum, (a - (sum - b_in_sum)) + (b - b_in_sum))
}
