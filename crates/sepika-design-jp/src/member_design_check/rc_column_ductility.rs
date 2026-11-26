use std::collections::HashMap;
use std::ops::RangeInclusive;

use sepika_core::face_distance::RcColumnGirderFaces;
use sepika_core::ids::ElemId;
use sepika_core::model::{Haunch, Model};
use sepika_core::section_shape::SectionShape;
use sepika_element::frame::beam::MemberForces;
use sepika_solver::statics::linear::RcColumnFaceEnds;
use serde::{Deserialize, Serialize};

use super::{is_rc_column_target, rc_column_faces};
use crate::design_position::{design_positions, prepared_end_positions};

/// RC柱の短期圧縮靱性推奨に対する設計者の適用判断。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RcColumnDuctilityApplicability {
    /// 未指定。標準値との参考比較は適否を示さない。
    #[default]
    Undetermined,
    /// 地震時に特に曲げ増大のおそれがあり、標準推奨値を用いる。
    Applicable,
    /// 設計者の判断により標準推奨の対象としない。
    NotApplicable,
    /// 実験根拠に基づく個別の上限を製品外で確認する。
    ExperimentalException,
}

/// RC柱1本の短期圧縮靱性推奨の付記。許容応力度の検定比・合否には影響しない。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RcColumnDuctilityAdvisory {
    pub elem: ElemId,
    pub applicability: RcColumnDuctilityApplicability,
    /// 無次元の危険断面位置（昇順）と比較結果。内力はその座標に完全一致する一意の行だけを使う。
    /// 非対象・実験根拠による例外では空。
    pub positions: Vec<(f64, RcColumnDuctilityComparison)>,
}

/// 1危険断面位置の平均圧縮軸応力度と標準推奨値の比較。
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum RcColumnDuctilityComparison {
    /// 有限の軸力がゼロまたは引張。圧縮の推奨値とは比較しない。
    NoCompression,
    /// 軸力または比較に必要な量を確認できない。
    Unavailable,
    Compared {
        /// 短期圧縮軸力をコンクリート全断面積で除した応力度 [N/mm²]。
        compression_n_per_mm2: f64,
        /// 靱性のための標準推奨値 [N/mm²]。材料の許容圧縮応力度とは異なる。
        standard_n_per_mm2: f64,
        /// 丸め誤差の範囲を除いて標準推奨値を超過した場合に true。
        exceeds_standard: bool,
    },
}

pub(super) fn collect(
    model: &Model,
    member_forces: &[(ElemId, MemberForces)],
    applicability: Option<&HashMap<ElemId, RcColumnDuctilityApplicability>>,
    geometries: &HashMap<ElemId, RcColumnGirderFaces>,
    face_responses: &HashMap<ElemId, Option<&RcColumnFaceEnds>>,
) -> Vec<RcColumnDuctilityAdvisory> {
    let mut by_elem = HashMap::with_capacity(member_forces.len());
    for (id, forces) in member_forces {
        by_elem
            .entry(*id)
            .and_modify(|sample| *sample = None)
            .or_insert(Some(forces));
    }

    model
        .elements
        .iter()
        .filter(|elem| is_rc_column_target(model, elem))
        .map(|elem| {
            let state = applicability
                .and_then(|map| map.get(&elem.id))
                .copied()
                .unwrap_or_default();
            let positions = match state {
                RcColumnDuctilityApplicability::NotApplicable
                | RcColumnDuctilityApplicability::ExperimentalException => Vec::new(),
                _ => {
                    let length_mm = model.member_length(elem);
                    let forces = by_elem.get(&elem.id).copied().flatten();
                    let geometry = geometries.get(&elem.id);
                    let positions = match geometry {
                        Some(geometry) => {
                            rc_column_faces::regular_positions(geometry, elem, model, length_mm)
                        }
                        None => design_positions(elem, model, length_mm),
                    };
                    let mut demands = positions
                        .into_iter()
                        .map(|xi| {
                            let axial_n = forces.and_then(|forces| {
                                let mut samples = forces.at.iter().filter(|(pos, _)| *pos == xi);
                                let (_, local) = samples.next()?;
                                samples.next().is_none().then_some(-local[0])
                            });
                            (xi, axial_n)
                        })
                        .collect::<Vec<_>>();
                    let mut face_distances_mm = [
                        Some(elem.rigid_zone.face_i_or_zero()),
                        Some(elem.rigid_zone.face_j_or_zero()),
                    ];
                    if let Some(geometry) = geometry {
                        let prepared = prepared_end_positions(elem, length_mm);
                        for (side, end) in geometry.ends.iter().enumerate() {
                            match end {
                                Some(Ok(face)) => {
                                    let xi = face.position_mm / face.length_mm;
                                    let axial_n = rc_column_faces::response(
                                        face_responses,
                                        elem.id,
                                        side,
                                        *face,
                                    )
                                    .ok()
                                    .map(|forces| -forces[0]);
                                    face_distances_mm[side] = Some(if side == 0 {
                                        face.position_mm
                                    } else {
                                        face.length_mm - face.position_mm
                                    });
                                    demands.push((xi, axial_n));
                                }
                                Some(Err(_)) => {
                                    face_distances_mm[side] = None;
                                    demands.push((prepared[side], None));
                                }
                                None => {}
                            }
                        }
                        demands.sort_by(|(a, _), (b, _)| a.total_cmp(b));
                    }
                    let haunches = model
                        .member_detail(elem.id)
                        .map_or([None; 2], |detail| [detail.haunch_i, detail.haunch_j]);
                    let nominal_range =
                        nominal_section_range(length_mm, face_distances_mm, haunches);
                    let area_mm2 = model
                        .element_section(elem)
                        .and_then(|section| section.shape.as_ref())
                        .and_then(gross_concrete_area_mm2);
                    let standard_n_per_mm2 = model
                        .element_material(elem)
                        .and_then(|mat| mat.fc)
                        .map(|fc| fc / 3.0)
                        .filter(|&standard| positive_finite(standard));
                    demands
                        .into_iter()
                        .map(|(xi, axial_n)| {
                            let area_mm2 = area_mm2.filter(|_| {
                                nominal_range
                                    .as_ref()
                                    .is_some_and(|range| range.contains(&xi))
                            });
                            (xi, compare(axial_n, area_mm2, standard_n_per_mm2))
                        })
                        .collect()
                }
            };
            RcColumnDuctilityAdvisory {
                elem: elem.id,
                applicability: state,
                positions,
            }
        })
        .collect()
}

fn positive_finite(value: f64) -> bool {
    value.is_finite() && value > 0.0
}

fn nominal_section_range(
    length_mm: f64,
    face_distances_mm: [Option<f64>; 2],
    haunches: [Option<Haunch>; 2],
) -> Option<RangeInclusive<f64>> {
    let haunches = haunches
        .map(|option| option.filter(|h| h.depth_increase != 0.0 || h.width_increase != 0.0));
    if haunches.iter().all(Option::is_none) {
        return Some(0.0..=1.0);
    }
    if !positive_finite(length_mm) {
        return None;
    }

    let mut inner_distances_mm = [0.0; 2];
    for (side, haunch) in haunches.into_iter().enumerate() {
        if let Some(h) = haunch {
            let face_mm = face_distances_mm[side]?;
            if !positive_finite(h.length)
                || !face_mm.is_finite()
                || face_mm < 0.0
                || ![h.depth_increase, h.width_increase]
                    .into_iter()
                    .all(|increase| increase.is_finite() && increase >= 0.0)
            {
                return None;
            }
            inner_distances_mm[side] = face_mm + h.length;
        }
    }
    if inner_distances_mm.iter().sum::<f64>() > length_mm {
        return None;
    }

    // 検定位置のクランプを使うと、中央をまたぐハンチを途中で打ち切ってしまう。
    let [i, j] = inner_distances_mm.map(|distance_mm| distance_mm / length_mm);
    Some(i..=1.0 - j)
}

fn gross_concrete_area_mm2(shape: &SectionShape) -> Option<f64> {
    let area_mm2 = match *shape {
        SectionShape::RcColumnRect { b, d, .. } if positive_finite(b) && positive_finite(d) => {
            b * d
        }
        SectionShape::RcColumnCircle { d, .. } if positive_finite(d) => {
            let radius_mm = d / 2.0;
            std::f64::consts::PI * radius_mm * radius_mm
        }
        _ => return None,
    };
    positive_finite(area_mm2).then_some(area_mm2)
}

fn compare(
    axial_n: Option<f64>,
    area_mm2: Option<f64>,
    standard_n_per_mm2: Option<f64>,
) -> RcColumnDuctilityComparison {
    use RcColumnDuctilityComparison::*;
    let Some(axial_n) = axial_n.filter(|n| n.is_finite()) else {
        return Unavailable;
    };
    if axial_n <= 0.0 {
        return NoCompression;
    }
    let (Some(area_mm2), Some(standard_n_per_mm2)) = (area_mm2, standard_n_per_mm2) else {
        return Unavailable;
    };
    let compression_n_per_mm2 = axial_n / area_mm2;
    if !positive_finite(compression_n_per_mm2) {
        return Unavailable;
    }
    let roundoff = (8.0 * f64::EPSILON) * compression_n_per_mm2.max(standard_n_per_mm2);
    Compared {
        compression_n_per_mm2,
        standard_n_per_mm2,
        exceeds_standard: compression_n_per_mm2 - standard_n_per_mm2 > roundoff,
    }
}
