//! ハンチ付きRC矩形梁の短期QD1用フェースと一般部の範囲。

use super::beam::{beam_supports_slab, BeamQd1Geometry};
use super::beam_slab_qd1::beam_supports_line_attached_slab;
use sepika_core::model::{ElementData, Haunch, Model};
use sepika_core::section_shape::SectionShape;

/// ハンチ付き端の方向別降伏曲げと、一般断面で検定できる範囲。
#[derive(Clone, Debug)]
pub struct BeamQd1Haunches {
    general_section_range_xi: Option<[f64; 2]>,
    moments_nmm: Result<[Option<[f64; 2]>; 2], String>,
}

impl BeamQd1Haunches {
    pub(super) fn end_moments_nmm(&self) -> Result<[Option<[f64; 2]>; 2], String> {
        self.moments_nmm.as_ref().copied().map_err(Clone::clone)
    }

    /// 無次元材軸座標が一般部でない、または一般部と確認できない場合に理由を返す。
    pub fn section_issue(&self, xi: f64) -> Option<String> {
        let reason = match self.general_section_range_xi {
            Some([i, j]) if (i..=j).contains(&xi) => return None,
            Some(_) => {
                "RC梁のハンチ区間や実フェース外では一般部の断面で曲げ・せん断・付着を検定できません"
            }
            None => "RC梁のハンチ区間を確定できず、一般部の断面を代用できません",
        };
        Some(match &self.moments_nmm {
            Err(issue) => format!("{reason}。{issue}"),
            Ok(_) => reason.into(),
        })
    }
}

fn haunch_ends(model: &Model, elem: &ElementData) -> [Option<Haunch>; 2] {
    model.member_detail(elem.id).map_or([None; 2], |detail| {
        [detail.haunch_i, detail.haunch_j].map(|end| {
            end.filter(|h| {
                h.width_increase != 0.0 || h.depth_increase != 0.0 || h.qd1_face.is_some()
            })
        })
    })
}

pub(crate) fn has_beam_qd1_haunch(model: &Model, elem: &ElementData) -> bool {
    haunch_ends(model, elem).iter().any(Option::is_some)
}

/// 短期QD1/min用のハンチ入力を、同じ梁のQD1幾何と照合する。
/// 形状変更のある端も入力のある端もなければ None。未確認の区間や材端には一般断面を代用しない。
pub fn beam_qd1_haunches(
    model: &Model,
    elem: &ElementData,
    geometry: Option<BeamQd1Geometry>,
) -> Option<BeamQd1Haunches> {
    let ends = haunch_ends(model, elem);
    if ends == [None; 2] {
        return None;
    }
    let range_xi = general_section_range(ends, elem, geometry);
    let moments_nmm = range_xi.as_ref().map_err(Clone::clone).and_then(|_| {
        let geometry = geometry.expect("範囲の確定にはQD1用の幾何が必要");
        let Some(SectionShape::RcBeamRect { b, d, .. }) = model
            .element_section(elem)
            .and_then(|section| section.shape.as_ref())
        else {
            return Err("QD1用のハンチ付き梁の一般部がRC矩形断面ではありません".into());
        };
        if beam_supports_slab(model, elem) || beam_supports_line_attached_slab(model, elem) {
            return Err("QD1用のハンチ付き梁では支持する床板の寄与を確定できません".into());
        }
        let moments = |index: usize| {
            ends[index]
                .map(|end| {
                    face_moments(end, geometry.face_distances_mm[index], *b, *d).map_err(|reason| {
                        let label = ["i端", "j端"][index];
                        format!("QD1用のハンチ付き梁の{label}: {reason}")
                    })
                })
                .transpose()
        };
        Ok([moments(0)?, moments(1)?])
    });
    Some(BeamQd1Haunches {
        general_section_range_xi: range_xi.ok(),
        moments_nmm,
    })
}

fn same_distance_or_dimension_mm(a_mm: f64, b_mm: f64) -> bool {
    a_mm.is_finite()
        && b_mm.is_finite()
        && (a_mm - b_mm).abs() <= 32.0 * f64::EPSILON * a_mm.abs().max(b_mm.abs()).max(1.0)
}

fn general_section_range(
    ends: [Option<Haunch>; 2],
    elem: &ElementData,
    geometry: Option<BeamQd1Geometry>,
) -> Result<[f64; 2], String> {
    let geometry = geometry.ok_or("QD1用のハンチ付き梁の実フェース間内法が確認できません")?;
    let recorded_faces_mm = [elem.rigid_zone.face_i, elem.rigid_zone.face_j];
    let mut lengths_mm = [0.0; 2];
    let mut inner_boundaries_xi = geometry.end_positions;
    for (index, end) in ends.into_iter().enumerate() {
        let Some(end) = end else { continue };
        let label = ["i端", "j端"][index];
        if geometry.uses_material_end[index] {
            return Err(format!(
                "QD1用のハンチ付き梁の{label}が偏心により実柱面と非交差です"
            ));
        }
        let face_mm = geometry.face_distances_mm[index];
        if !recorded_faces_mm[index].is_some_and(|recorded_mm| {
            recorded_mm >= 0.0 && same_distance_or_dimension_mm(recorded_mm, face_mm)
        }) {
            return Err(format!(
                "QD1用のハンチ付き梁の{label}でハンチ長の起点と実フェースが一致しません"
            ));
        }
        if !end.length.is_finite() || end.length <= 0.0 {
            return Err(format!(
                "QD1用のハンチ付き梁の{label}のハンチ長が有限の正値ではありません"
            ));
        }
        lengths_mm[index] = end.length;
        let from_end_xi = (face_mm + end.length) / geometry.axis_length_mm;
        inner_boundaries_xi[index] = if index == 0 {
            from_end_xi
        } else {
            1.0 - from_end_xi
        };
    }
    if lengths_mm[0] + lengths_mm[1] >= geometry.clear_span_mm
        || inner_boundaries_xi[0] >= inner_boundaries_xi[1]
    {
        return Err("QD1用のハンチ付き梁でハンチ区間が重なるか、一般部を挟んでいません".into());
    }
    Ok(inner_boundaries_xi)
}

fn face_moments(
    haunch: Haunch,
    physical_distance_mm: f64,
    base_width_mm: f64,
    base_depth_mm: f64,
) -> Result<[f64; 2], &'static str> {
    let face = haunch
        .qd1_face
        .ok_or("確認済みの矩形材端フェースと上下の有効主筋が未入力です")?;
    if let Some(reason) = face.input_issue() {
        return Err(reason);
    }
    if ![haunch.width_increase, haunch.depth_increase]
        .into_iter()
        .all(|value| value.is_finite() && value >= 0.0)
        || (haunch.width_increase == 0.0 && haunch.depth_increase == 0.0)
    {
        return Err("ハンチの幅・せい増分が有限の非負値で、少なくとも一方が正の必要があります");
    }
    if !same_distance_or_dimension_mm(face.distance_mm, physical_distance_mm) {
        return Err("入力したフェース距離がQD1用の実フェース距離と一致しません");
    }
    if !same_distance_or_dimension_mm(face.width_mm, base_width_mm + haunch.width_increase)
        || !same_distance_or_dimension_mm(face.depth_mm, base_depth_mm + haunch.depth_increase)
    {
        return Err("矩形フェースの実幅・実せいが一般部とハンチ増分の合計に一致しません");
    }
    let moments_nmm = [face.top, face.bottom]
        .map(|group| 0.9 * group.area_mm2 * group.yield_strength_n_mm2 * group.effective_depth_mm);
    if !moments_nmm.into_iter().all(|my| my.is_finite() && my > 0.0) {
        return Err("上下引張の降伏曲げモーメントを有限の正値として算定できません");
    }
    Ok(moments_nmm)
}
