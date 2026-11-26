use crate::adjacency::NodeAdjacency;
use crate::geom::vec3::{cross, dist, dot, local_frame_axes, norm, scale, sub, unit};
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
}

pub struct RcColumnGirderFaces {
    /// i/j順。大梁の確認を要しない端はNone、未確定な端は理由付きエラー。
    pub ends: [Option<Result<RcColumnGirderFace, String>>; 2],
    /// 確定した両面が一致または逆転するときはfalse。
    pub has_positive_span: bool,
}

/// 柱軸に直交する外面を持つ矩形大梁の実配置と、その面に垂直な局所軸の添字。
pub fn aligned_girder(
    model: &Model,
    girder: ElemId,
    inward: [f64; 3],
) -> Result<(BeamPlacement, usize), String> {
    let placement = model.calculation_beam_placement(SupportMemberId::Primary(girder))?;
    if matches!(
        placement.section_shape,
        SectionShape::RcBeamRect { .. } | SectionShape::SrcBeamRect { .. }
    ) {
        if let Some(normal) =
            (1..=2).find(|&i| norm(cross(placement.axes[i], inward)) <= 32.0 * f64::EPSILON)
        {
            return Ok((placement, normal));
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
        let [a, b] = match model.primary_frame_axis_ends(elem)? {
            Some(ends) => ends,
            None => [
                model.node(i).ok_or("柱のi端節点がありません")?.coord,
                model.node(j).ok_or("柱のj端節点がありません")?.coord,
            ],
        };
        let length_mm = dist(a, b);
        let axes = local_frame_axes(a, b, elem.local_axis.ref_vector)?;
        Ok::<_, String>(([a, b], axes, length_mm))
    })();
    let nodes = [i, j];
    let mut faces = std::array::from_fn(|side| {
        let joint = adjacency.elements_at(model, nodes[side]).filter(|other| {
            other.id != elem.id
                && !model
                    .element_section(other)
                    .is_some_and(|section| section.frame_use == Some(FrameSectionUse::Column))
        });
        let mut distance_mm: Option<f64> = None;
        for other in joint {
            let resolve = || {
                let (ends, axes, _) = axes_at_ends.as_ref().map_err(Clone::clone)?;
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
                check_continuation(model, elem, adjacency, nodes[side], ends[side], inward)?;
                let (beam, normal) = aligned_girder(model, other.id, inward)?;
                let face_mm = contact_distance(shape, &beam, normal, ends[side], *axes, inward)?;
                if distance_mm.is_some_and(|other| other != face_mm) {
                    return Err("接続大梁によって材端フェースの位置が異なります".into());
                }
                Ok(face_mm)
            };
            match resolve() {
                Ok(face_mm) => distance_mm = Some(face_mm),
                Err(reason) => return Some(Err(reason)),
            }
        }
        distance_mm.map(|distance_mm| {
            let length_mm = axes_at_ends.as_ref().expect("確認済みの柱材軸").2;
            Ok(RcColumnGirderFace {
                position_mm: if side == 0 {
                    distance_mm
                } else {
                    length_mm - distance_mm
                },
                length_mm,
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
    origin: [f64; 3],
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
        let ends = match model.primary_frame_axis_ends(other)? {
            Some(ends) => ends,
            None => [a, b].map(|id| model.node(id).map_or([f64::NAN; 3], |node| node.coord)),
        };
        let side = usize::from(a != node);
        let direction = unit(sub(ends[1 - side], ends[side]));
        if ends[side] != origin
            || direction.is_none_or(|dir| {
                norm(cross(dir, inward)) > 32.0 * f64::EPSILON || dot(dir, inward) >= 0.0
            })
        {
            return Err("柱の材軸が連続しない接合です".into());
        }
    }
    Ok(())
}

fn contact_distance(
    column: &SectionShape,
    beam: &BeamPlacement,
    normal: usize,
    column_end: [f64; 3],
    column_axes: [[f64; 3]; 3],
    inward: [f64; 3],
) -> Result<f64, String> {
    let (b, d) = match beam.section_shape {
        SectionShape::RcBeamRect { b, d, .. } | SectionShape::SrcBeamRect { b, d, .. } => (b, d),
        _ => unreachable!(),
    };
    let beam_half = [
        dist(beam.actual_ends[0], beam.actual_ends[1]) / 2.0,
        d / 2.0,
        b / 2.0,
    ];
    let transverse = 3 - normal;
    let delta = sub(beam.actual_ends[0], column_end);
    let center_normal = dot(delta, inward);
    let distance_mm = center_normal + beam_half[normal];
    let axes = [beam.axes[0], beam.axes[transverse]];
    let half = [beam_half[0], beam_half[transverse]];
    let offset = [dot(delta, axes[0]) + half[0], dot(delta, axes[1])];
    let valid = |x: f64| x.is_finite() && x > 0.0;
    let overlaps = match *column {
        SectionShape::RcColumnCircle { d, .. } => {
            valid(d)
                && (offset[0].abs() - half[0])
                    .max(0.0)
                    .hypot((offset[1].abs() - half[1]).max(0.0))
                    < d / 2.0
        }
        SectionShape::RcColumnRect { b, d, .. } => {
            let cosines = axes.map(|axis| [dot(axis, column_axes[1]), dot(axis, column_axes[2])]);
            let column_half = [d / 2.0, b / 2.0];
            valid(b)
                && valid(d)
                && (0..2).all(|k| {
                    let [c, s] = cosines[k];
                    offset[k].abs() < half[k] + c.abs() * column_half[0] + s.abs() * column_half[1]
                })
                && (0..2).all(|k| {
                    let [c, s] = [cosines[0][k], cosines[1][k]];
                    (offset[0] * c + offset[1] * s).abs()
                        < column_half[k] + c.abs() * half[0] + s.abs() * half[1]
                })
        }
        _ => unreachable!(),
    };
    if !beam_half.into_iter().all(valid)
        || !distance_mm.is_finite()
        || distance_mm < 0.0
        || center_normal > beam_half[normal]
        || !overlaps
    {
        let SupportMemberId::Primary(id) = beam.member else {
            unreachable!()
        };
        return Err(format!(
            "大梁 ID {} の実外形と柱の実材端が正の面積で接触しません",
            id.0
        ));
    }
    Ok(distance_mm)
}
