//! ハンチ付きRC矩形梁の短期QD1用フェースと一般部の範囲。

use super::beam::{beam_supports_slab, BeamQd1Geometry};
use super::beam_slab_qd1::beam_supports_line_attached_slab;
use super::{axis_props_from_shape, shear_alpha};
use crate::design_position::same_qd1_station;
use crate::ultimate::rc_props::RcDirection;
use sepika_core::model::{ElementData, Haunch, Model};
use sepika_core::section_shape::SectionShape;

/// ハンチ付き端の方向別降伏曲げと、一般断面で検定できる範囲。
#[derive(Clone, Debug)]
pub struct BeamQd1Haunches {
    general_section_range: Option<GeneralSectionRange>,
    actual_faces_xi: Option<[f64; 2]>,
    general_depths_mm: [Option<f64>; 2],
    sections: Result<[Option<ValidatedFace>; 2], String>,
    alpha: Result<f64, &'static str>,
    mixed_response: bool,
}

#[derive(Clone, Copy, Debug)]
struct ValidatedFace {
    moments_nmm: [f64; 2],
    depths_mm: [f64; 2],
}

#[derive(Clone, Debug)]
struct GeneralSectionRange {
    actual_bounds_xi: [f64; 2],
    recorded_haunch_ends_xi: [Option<f64>; 2],
}

impl GeneralSectionRange {
    fn contains(&self, xi: f64) -> bool {
        let [i, j] = self.actual_bounds_xi;
        (i..=j).contains(&xi) || self.recorded_haunch_ends_xi.contains(&Some(xi))
    }
}

impl BeamQd1Haunches {
    pub(super) fn end_moments_nmm(&self) -> Result<[Option<[f64; 2]>; 2], String> {
        self.sections
            .as_ref()
            .map(|sections| sections.map(|face| face.map(|face| face.moments_nmm)))
            .map_err(Clone::clone)
    }

    /// 同じ確定応答の解析断面力をαに使う。異時刻の包絡しかない場合は None を渡す。
    pub fn set_shear_span_reference(&mut self, rows: Option<&[(f64, [f64; 6])]>) {
        self.mixed_response = rows.is_none();
        self.alpha = rows.map_or(
            Err("異時刻の内力包絡では、同じ確定ステップの曲げ・せん断力を確認できません"),
            |rows| self.resolve_shear_alpha(rows),
        );
    }

    pub(super) fn shear_alpha(&self) -> Result<f64, &'static str> {
        self.alpha
    }

    pub(super) fn mixes_response_steps(&self) -> bool {
        self.mixed_response
    }

    fn resolve_shear_alpha(&self, rows: &[(f64, [f64; 6])]) -> Result<f64, &'static str> {
        let [i, j] = self
            .actual_faces_xi
            .ok_or("実フェース間の位置を確認できません")?;
        let has_unique_row = |xi| {
            rows.iter()
                .filter(|(pos, _)| same_qd1_station(xi, *pos))
                .take(2)
                .count()
                == 1
        };
        if [i, j].into_iter().any(|xi| !has_unique_row(xi)) {
            return Err("実フェースの応答が欠落または重複しています");
        }
        if rows.iter().any(|(xi, _)| !xi.is_finite()) {
            return Err("解析断面の位置が非有限です");
        }
        let in_clear_span =
            |xi| (i..=j).contains(&xi) || same_qd1_station(xi, i) || same_qd1_station(xi, j);
        let physical_rows = || rows.iter().filter(|(xi, _)| in_clear_span(*xi));
        let mut max_mz_nmm: f64 = 0.0;
        for (_, force) in physical_rows() {
            if !force[5].is_finite() {
                return Err("実フェース間の解析曲げモーメントが非有限です");
            }
            max_mz_nmm = max_mz_nmm.max(force[5].abs());
        }
        let mut min_alpha: f64 = 2.0;
        for &(xi, force) in physical_rows().filter(|(_, force)| force[5].abs() == max_mz_nmm) {
            if !has_unique_row(xi) {
                return Err("最大曲げ位置の解析断面が重複しています");
            }
            let qy_n = force[1];
            if !qy_n.is_finite() {
                return Err("最大曲げ位置の解析せん断力が非有限です");
            }
            let tension_side = usize::from(force[5] >= 0.0);
            let depth_mm = self
                .depth_for_alpha(xi, tension_side)
                .filter(|d| d.is_finite() && *d > 0.0)
                .ok_or("最大曲げ位置・引張側の有効せいを確認できません")?;
            let alpha = shear_alpha(max_mz_nmm, qy_n, depth_mm, 2.0);
            if !alpha.is_finite() {
                return Err("最大曲げ位置のせん断スパン比が非有限です");
            }
            min_alpha = min_alpha.min(alpha);
        }
        Ok(min_alpha)
    }

    fn depth_for_alpha(&self, xi: f64, tension_side: usize) -> Option<f64> {
        if self
            .general_section_range
            .as_ref()
            .is_some_and(|range| range.contains(xi))
        {
            return self.general_depths_mm[tension_side];
        }
        let index = self
            .actual_faces_xi?
            .into_iter()
            .position(|pos| same_qd1_station(pos, xi))?;
        match self.sections.as_ref().ok()?[index] {
            Some(face) => Some(face.depths_mm[tension_side]),
            None => self.general_depths_mm[tension_side],
        }
    }

    /// 無次元材軸座標が一般部でない、または一般部と確認できない場合に理由を返す。
    pub fn section_issue(&self, xi: f64) -> Option<String> {
        let reason = match &self.general_section_range {
            Some(range) if range.contains(xi) => return None,
            Some(_) => {
                "RC梁のハンチ区間や実フェース外では一般部の断面で曲げ・せん断・付着を検定できません"
            }
            None => "RC梁のハンチ区間を確定できず、一般部の断面を代用できません",
        };
        Some(match &self.sections {
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
    let range = general_section_range(ends, elem, geometry);
    let shape = model
        .element_section(elem)
        .and_then(|section| section.shape.as_ref());
    let general_depths_mm = [true, false].map(|tension_is_top| {
        let props = axis_props_from_shape(shape?, RcDirection::Strong, tension_is_top)?;
        (props.at > 0.0 && props.at.is_finite()).then_some(props.d)
    });
    let sections = range.as_ref().map_err(Clone::clone).and_then(|_| {
        let geometry = geometry.expect("範囲の確定にはQD1用の幾何が必要");
        let Some(SectionShape::RcBeamRect { b, d, .. }) = shape else {
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
        general_section_range: range.ok(),
        actual_faces_xi: geometry.map(|geometry| geometry.end_positions),
        general_depths_mm,
        sections,
        alpha: Err("同じ確定応答の曲げ・せん断力を確認できません"),
        mixed_response: false,
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
) -> Result<GeneralSectionRange, String> {
    let geometry = geometry.ok_or("QD1用のハンチ付き梁の実フェース間内法が確認できません")?;
    let recorded_faces_mm = [elem.rigid_zone.face_i, elem.rigid_zone.face_j];
    let mut lengths_mm = [0.0; 2];
    let mut inner_boundaries_xi = geometry.end_positions;
    let mut recorded_haunch_ends_xi = [None; 2];
    for (index, end) in ends.into_iter().enumerate() {
        let Some(end) = end else { continue };
        let label = ["i端", "j端"][index];
        if geometry.uses_material_end[index] {
            return Err(format!(
                "QD1用のハンチ付き梁の{label}が偏心により実柱面と非交差です"
            ));
        }
        let face_mm = geometry.face_distances_mm[index];
        let Some(recorded_mm) = recorded_faces_mm[index].filter(|&recorded_mm| {
            recorded_mm >= 0.0 && same_distance_or_dimension_mm(recorded_mm, face_mm)
        }) else {
            return Err(format!(
                "QD1用のハンチ付き梁の{label}でハンチ長の起点と実フェースが一致しません"
            ));
        };
        if !end.length.is_finite() || end.length <= 0.0 {
            return Err(format!(
                "QD1用のハンチ付き梁の{label}のハンチ長が有限の正値ではありません"
            ));
        }
        lengths_mm[index] = end.length;
        let haunch_end = |start_mm| {
            let from_end_xi = (start_mm + end.length) / geometry.axis_length_mm;
            if index == 0 {
                from_end_xi
            } else {
                1.0 - from_end_xi
            }
        };
        inner_boundaries_xi[index] = haunch_end(face_mm);
        let recorded_end_xi = haunch_end(recorded_mm);
        if [inner_boundaries_xi[index], recorded_end_xi]
            .into_iter()
            .any(|xi| same_qd1_station(xi, geometry.end_positions[index]))
        {
            return Err(format!(
                "QD1用のハンチ付き梁の{label}で材端フェースとハンチ端を区別できません"
            ));
        }
        recorded_haunch_ends_xi[index] = Some(recorded_end_xi);
    }
    if lengths_mm[0] + lengths_mm[1] >= geometry.clear_span_mm
        || inner_boundaries_xi[0] >= inner_boundaries_xi[1]
    {
        return Err("QD1用のハンチ付き梁でハンチ区間が重なるか、一般部を挟んでいません".into());
    }
    Ok(GeneralSectionRange {
        actual_bounds_xi: inner_boundaries_xi,
        recorded_haunch_ends_xi,
    })
}

fn face_moments(
    haunch: Haunch,
    physical_distance_mm: f64,
    base_width_mm: f64,
    base_depth_mm: f64,
) -> Result<ValidatedFace, &'static str> {
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
    Ok(ValidatedFace {
        moments_nmm,
        depths_mm: [face.top.effective_depth_mm, face.bottom.effective_depth_mm],
    })
}
