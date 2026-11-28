use super::{distributed_intensity, resolve, Comp, FrameDeadLoadMap, LoadedFrameSection};
use crate::frame::beam::MemberForces;
use crate::transform::LocalFrame;
use sepika_core::geom::vec3::{dist, local_frame_axes_and_fallback_reference};
use sepika_core::model::{MemberLoad, MemberLoadKind};

mod exact;
pub(super) use exact::has_nonzero_point_jump;
use exact::ExactSpanError;

/// 補剛区間の符号付き強軸曲げ分布と、指定した需要範囲の断面力。
pub struct FrameStrongBendingSpan {
    /// 補剛区間のi/j側端の曲げモーメント [N·mm]。
    pub end_moments_nmm: [f64; 2],
    /// 符号付き曲げが補剛区間で非減少または非増加と確認できた場合にtrue。
    pub monotonic: bool,
    /// 区間内部で端部以上の曲げ絶対値を確認できた場合にtrue。
    pub interior_maximum: bool,
    /// 需要範囲の非零の最大平坦部で軸力と弱軸曲げの一定性を確認できない場合にtrue。
    pub unverified_maximum_plateau: bool,
    /// 時刻歴で強軸曲げの分布を確定できない場合の理由。
    pub unverified_distribution: Option<&'static str>,
    /// 需要範囲で絶対値最大に達する断面。複数区間の照査では需要範囲の両端も含む。
    /// 時刻歴の分布が未確認の場合は照合済みの断面力のみを含むことがある。
    pub demand_peaks: Vec<FrameStrongBendingPeak>,
}

/// 強軸曲げの需要を照査する全長比と、その位置の局所断面力 [N, N·mm]。
pub struct FrameStrongBendingPeak {
    pub xi: f64,
    /// 実材軸i端からの距離 [mm]。
    pub x_mm: f64,
    /// 集中荷重位置では左極限。
    pub left: [f64; 6],
    /// i端フェースまたは区間内部で集中荷重による断面力の跳びがある場合の右極限。
    /// 断面力が非有限なら理由付きエラー。
    pub right: Result<Option<[f64; 6]>, String>,
    /// 補剛端または材端フェースで決まる需要範囲のi側端。
    pub is_i_face: bool,
    /// 補剛端または材端フェースで決まる需要範囲のj側端。
    pub is_j_face: bool,
}

/// 線形静的応答の実材軸全長で確認した符号付き強軸曲げと、同一切断面の断面力。
pub struct FrameStaticStrongBending {
    /// i/j実材端の強軸曲げ [N·mm]。不明なら NaN。
    pub end_moments_nmm: [f64; 2],
    /// 回復値、保存せん断力、保存曲げの単調方向が実材軸全長で一致する場合にtrue。
    pub monotonic: bool,
    /// 絶対値最大が内部にも存在するならtrue。
    pub interior_maximum: bool,
    /// 区間で非零の強軸曲げを確認した場合にtrue。
    pub has_strong_bending: bool,
    /// 回復または照合済み保存値を起点とする分布で、区間内の曲げゼロ位置に対応する圧縮軸力 [N]。
    /// 軸力・弱軸曲げの区間一定性を確認できない場合には使用しない。
    pub compression_at_zero_crossing_n: Option<f64>,
    /// 材端、荷重境界、曲げの内部極値を実材軸i端から昇順に返す。集中荷重位置では切断側を区別する。
    pub sections: Vec<FrameStaticBendingSection>,
    /// 軸力の変動、弱軸せん断、引張と二軸曲げの併存で、全区間の組合せを確定できない場合の理由。
    pub combination_issue: Option<&'static str>,
}

/// 静的照査位置の回復値と、同じ位置の保存断面力。
pub struct FrameStaticBendingSection {
    /// 回復した照査位置と切断側の断面力。保存値と同じ荷重から確認できない成分は NaN。
    pub recovered: FrameStrongBendingPeak,
    /// 回復した切断位置と厳密に一致する保存断面力の左極限 [N, N·mm]。
    pub saved_left: Option<[f64; 6]>,
    /// 同位置の保存左極限に集中荷重の跳びを加えた右極限 [N, N·mm]。回復値に右極限がある場合のみ。
    pub saved_right: Result<Option<[f64; 6]>, String>,
    /// 保存値または回復値のせん断がゼロで、隣接域の強軸せん断の連続極限を照査できる切断側。左・右の順。
    /// `recovered.right` がなければ、共通の切断面に対する判定を左に返す。
    pub zero_shear_limits: [bool; 2],
}

/// 静的な強軸曲げ分布の算定に失敗した段階と理由。
pub enum FrameStaticBendingError {
    /// 同じ実材軸の応答として保存断面力を照合できない。
    Stations(String),
    /// 断面力の矛盾は検出されていないが、区間全体の分布を確認できない。
    Distribution(String),
}

/// 線形静的応答と同じ部材荷重・係数および両端を含む保存断面力から実材軸全長の強軸曲げを確認する。
/// 区間座標は実材軸i端を起点とする [mm]。断面力の不整合、荷重の投影・曲げ分布を確認できない場合はエラー。
/// 非有限な保存成分で全体の組合せが未確認でも、独立に照合できた成分の回復値を返すことがある。
pub fn frame_static_strong_bending(
    [i, j]: [[f64; 3]; 2],
    reference: [f64; 3],
    loads: &[(f64, &MemberLoad)],
    forces: &MemberForces,
) -> Result<FrameStaticStrongBending, FrameStaticBendingError> {
    use FrameStaticBendingError::{Distribution, Stations};
    let (rot, fallback) = local_frame_axes_and_fallback_reference(i, j, reference)
        .map_err(|reason| Stations(reason.into()))?;
    let frame = LocalFrame { rot };
    let length_mm = dist(i, j);
    if forces.at.iter().any(|(xi, _)| !xi.is_finite()) {
        return Err(Stations("保存断面力の評価位置が非有限です".into()));
    }
    let duplicate = forces.at.windows(2).any(|w| w[0].0 == w[1].0);
    let recover_error = if duplicate { Stations } else { Distribution };
    let stations = bending_stations_mm(length_mm, forces).map_err(recover_error)?;
    let mut positions = Vec::new();
    let mut repeated_positions_mm = Vec::new();
    let response_stations = if duplicate {
        for (i, pair) in forces.at.windows(2).enumerate() {
            if pair[0].0 == pair[1].0 {
                if pair[0]
                    .1
                    .iter()
                    .zip(pair[1].1)
                    .any(|(a, b)| a.is_finite() && b.is_finite() && *a != b)
                {
                    return Err(Stations(
                        "重複した測点の保存断面力が一致せず、解析断面力と部材荷重の釣合いを確認できません".into(),
                    ));
                }
                if pair
                    .iter()
                    .flat_map(|(_, force)| force)
                    .any(|v| !v.is_finite())
                {
                    return Err(Stations(
                        "重複した測点の保存断面力に非有限な成分があり、同値であることを確認できません".into(),
                    ));
                }
                repeated_positions_mm.push(stations[i].0);
            }
        }
        positions.extend(stations.iter().enumerate().filter_map(|(index, &station)| {
            (index == 0 || forces.at[index - 1].0 != forces.at[index].0).then_some(station)
        }));
        &positions
    } else {
        &stations
    };
    for &(factor, load) in loads {
        let input = match load.kind {
            MemberLoadKind::Point { p, .. } => [p, 0.0],
            MemberLoadKind::Distributed { w1, w2, .. } => [w1, w2],
        };
        if factor != 0.0 && input.into_iter().any(|p| p != 0.0 && factor * p == 0.0) {
            return Err(recover_error(
                "荷重係数を適用した部材荷重の強度を保持できません".into(),
            ));
        }
    }
    let response =
        frame_bending_response_mm(&frame, length_mm, loads, response_stations).map_err(Stations)?;
    let duplicate_reason =
        "保存断面力の評価位置が重複しているため、補剛区間全体の組合せを確定できません";
    let verify_projections = || {
        response
            .verify_static_load_projections([i, j], fallback.unwrap_or(reference))
            .map_err(recover_error)
    };
    let recover_sections = |response, partial| {
        let mut span = StrongBendingResponse::new(&frame, length_mm, response, response_stations)
            .exact_static_sections(partial)?;
        if duplicate {
            span.sections
                .retain(|cut| !repeated_positions_mm.contains(&cut.recovered.x_mm));
            span.monotonic = false;
            span.interior_maximum = false;
            span.combination_issue = Some(duplicate_reason);
        }
        Ok(span)
    };
    if stations
        .iter()
        .any(|(_, row)| row.iter().any(|value| !value.is_finite()))
    {
        let projections_confirmed = match verify_projections() {
            Ok(()) => true,
            Err(reason) if duplicate => return Err(reason),
            Err(_) => false,
        };
        let load_response = LoadedFrameSection::new(response.map, [0.0; 12]);
        load_response
            .verify_partial_bending_stations(&stations)
            .map_err(Stations)?;
        let reason = if duplicate {
            "保存断面力の評価位置が重複し、非有限な成分もあるため、区間全体の組合せを確定できません"
        } else {
            "保存断面力に非有限な成分があるため、区間全体の組合せを確定できません"
        };
        if projections_confirmed {
            let response = LoadedFrameSection::new(load_response.map, response.node_force_local);
            if let Ok(mut span) = recover_sections(&response, true) {
                span.combination_issue = Some(reason);
                return Ok(span);
            }
        }
        return Err(Distribution(reason.into()));
    }
    verify_bending_stations(&response, &stations).map_err(Stations)?;
    verify_projections()?;
    verify_bending_stations_with(&stations, |x_mm| response.exact_static_cut_mm(x_mm))
        .map_err(|reason| Stations(format!("厳密な回復値。{reason}")))?;
    recover_sections(&response, false).map_err(|error| {
        let reason = match error {
            ExactSpanError::VaryingStrongLoad => {
                "強軸方向の合算分布荷重の強度が区間内で変化するため、曲げ分布を確定できません"
            }
            ExactSpanError::Unconfirmed => {
                "強軸曲げの分布と照合位置の断面力を有限値として確定できません"
            }
        };
        Distribution(if duplicate {
            format!("{duplicate_reason}。{reason}")
        } else {
            reason.into()
        })
    })
}

/// 同じ線形静的応答に使った全長の部材荷重・係数と両端を含む断面力から、強軸曲げを回復する。
/// 最大値を求める区間はi端からの距離 [mm] で指定する。入力断面力と釣合いが一致しない場合は理由付きエラー。
pub fn frame_strong_bending_span(
    frame: &LocalFrame,
    length_mm: f64,
    [demand_lo, demand_hi]: [f64; 2],
    loads: &[(f64, &MemberLoad)],
    forces: &MemberForces,
) -> Result<FrameStrongBendingSpan, String> {
    let stations_mm = bending_stations_mm(length_mm, forces)?;
    frame_strong_bending_span_mm(
        frame,
        length_mm,
        [demand_lo, demand_hi],
        loads,
        &stations_mm,
    )
}

fn bending_stations_mm(
    length_mm: f64,
    forces: &MemberForces,
) -> Result<Vec<(f64, [f64; 6])>, String> {
    if forces.at.first().is_none_or(|(xi, _)| *xi != 0.0) {
        return Err("i端の解析断面力がありません".into());
    }
    if forces.at.last().is_none_or(|(xi, _)| *xi != 1.0) {
        return Err("j端の解析断面力がありません".into());
    }
    Ok(forces
        .at
        .iter()
        .map(|&(xi, force)| (xi * length_mm, force))
        .collect())
}

/// 実材軸i端起点の境界 [mm] で区切った各区間の符号付き強軸曲げ分布を確認する。
/// 境界には両実材端を含める。全長の荷重と断面力を照合し、指定需要範囲で各区間の両端・最大を評価する。
pub fn frame_strong_bending_intervals(
    frame: &LocalFrame,
    length_mm: f64,
    demand_extent_mm: [f64; 2],
    boundaries_mm: &[f64],
    loads: &[(f64, &MemberLoad)],
    forces: &MemberForces,
) -> Result<Vec<FrameStrongBendingSpan>, String> {
    let [lo, hi] = demand_extent_mm;
    if !(0.0 <= lo && lo < hi && hi <= length_mm)
        || boundaries_mm.len() < 2
        || boundaries_mm.first() != Some(&0.0)
        || boundaries_mm.last() != Some(&length_mm)
    {
        return Err("補剛区間の境界または需要範囲が不正です".into());
    }
    let stations = bending_stations_mm(length_mm, forces)?;
    let response = verified_frame_bending_response_mm(frame, length_mm, loads, &stations)?;
    let bending = StrongBendingResponse::new(frame, length_mm, &response, &stations);
    boundaries_mm
        .windows(2)
        .map(|w| {
            let interval = [w[0], w[1]];
            let demand = [w[0].max(demand_extent_mm[0]), w[1].min(demand_extent_mm[1])];
            bending.span(interval, demand, true)
        })
        .collect()
}

/// i端からの実距離 [mm] で両端と保存断面力を照合する。集中部材荷重と同じ位置の断面力は左極限とする。
/// 照合位置は昇順かつ重複なしとし、入力断面力と釣合いが一致しない場合は理由付きエラーを返す。
pub fn frame_strong_bending_span_mm(
    frame: &LocalFrame,
    length_mm: f64,
    [demand_lo, demand_hi]: [f64; 2],
    loads: &[(f64, &MemberLoad)],
    stations_mm: &[(f64, [f64; 6])],
) -> Result<FrameStrongBendingSpan, String> {
    let response = verified_frame_bending_response_mm(frame, length_mm, loads, stations_mm)?;
    StrongBendingResponse::new(frame, length_mm, &response, stations_mm).span(
        [0.0, length_mm],
        [demand_lo, demand_hi],
        false,
    )
}

struct StrongBendingResponse<'a> {
    response: &'a LoadedFrameSection,
    length_mm: f64,
    strong_loads: Vec<Comp>,
    breaks: Vec<f64>,
    stations_mm: &'a [(f64, [f64; 6])],
}

impl<'a> StrongBendingResponse<'a> {
    fn new(
        frame: &LocalFrame,
        length_mm: f64,
        response: &'a LoadedFrameSection,
        stations_mm: &'a [(f64, [f64; 6])],
    ) -> Self {
        let loads = &response.map.loads;
        let strong_loads = loads
            .iter()
            .filter_map(|load| resolve(load, frame)[1])
            .collect();
        let mut breaks = vec![];
        for load in loads {
            match load.kind {
                MemberLoadKind::Point { a, .. } => breaks.push(a),
                MemberLoadKind::Distributed { a, b, .. } => breaks.extend([a, b]),
            }
        }
        breaks.sort_by(f64::total_cmp);
        breaks.dedup();
        Self {
            response,
            length_mm,
            strong_loads,
            breaks,
            stations_mm,
        }
    }

    fn observed_shear_y(&self, x_mm: f64) -> Option<f64> {
        let index = self.stations_mm.partition_point(|(x, _)| *x < x_mm);
        self.stations_mm
            .get(index)
            .filter(|(x, _)| *x == x_mm)
            .map(|(_, observed)| observed[1])
    }

    fn observed_shear_sign_matches(&self, x_mm: f64, jump_n: f64, recovered_n: f64) -> bool {
        self.observed_shear_y(x_mm).is_none_or(|left| {
            let observed_n = left + jump_n;
            observed_n.is_finite() && observed_n.partial_cmp(&0.0) == recovered_n.partial_cmp(&0.0)
        })
    }

    fn span(
        &self,
        [span_lo, span_hi]: [f64; 2],
        [demand_lo, demand_hi]: [f64; 2],
        include_ends: bool,
    ) -> Result<FrameStrongBendingSpan, String> {
        if !(0.0 <= span_lo
            && span_lo <= demand_lo
            && demand_lo < demand_hi
            && demand_hi <= span_hi
            && span_hi <= self.length_mm)
        {
            return Err("強軸曲げの需要を評価する区間が不正です".into());
        }
        let response = self.response;
        let start = response.at_mm(span_lo)?;
        let end = response.at_mm(span_hi)?;
        // 保存点は左極限なので、集中荷重を跨ぐ場合はそのままi端の区間内側と比較できない。
        let i_jump_n = response.point_load_jump(span_lo)[1];
        let boundary_shear_conflicts = include_ends
            && !(self.observed_shear_sign_matches(span_lo, i_jump_n, start[1] + i_jump_n)
                && self.observed_shear_sign_matches(span_hi, 0.0, end[1]));
        let mut candidates = vec![
            (demand_lo, response.at_mm(demand_lo)?),
            (demand_hi, response.at_mm(demand_hi)?),
        ];
        let end_abs = start[5].abs().max(end[5].abs());
        let mut has_positive_shear = false;
        let mut has_negative_shear = false;
        let mut uncertain_shear_sign = false;
        let mut interior_maximum = false;
        let mut unverified_flat_maximum_nmm = None::<f64>;
        let mut record_candidate = |x_mm: f64, on_plateau: bool| {
            if !(span_lo < x_mm && x_mm < span_hi) {
                return Ok(());
            }
            let force = response.at_mm(x_mm)?;
            let abs = force[5].abs();
            let discernible = 64.0 * f64::EPSILON * abs.max(end_abs);
            interior_maximum |= abs - end_abs > discernible || (on_plateau && abs == end_abs);
            if !on_plateau && demand_lo < x_mm && x_mm < demand_hi {
                candidates.push((x_mm, force));
            }
            Ok::<_, String>(())
        };
        let inner_breaks = &self.breaks[self.breaks.partition_point(|&x| x <= span_lo)
            ..self.breaks.partition_point(|&x| x < span_hi)];
        let mut lo = span_lo;
        for hi in inner_breaks.iter().copied().chain([span_hi]) {
            if lo > span_lo {
                record_candidate(lo, false)?;
            }
            let width = hi - lo;
            let q0 = response.at_mm(lo)?[1] + response.point_load_jump(lo)[1];
            let mut w_lo = 0.0;
            let mut w_hi = 0.0;
            for &load in &self.strong_loads {
                match load {
                    Comp::Dist { a, b, w1, w2 } if a <= lo && hi <= b => {
                        w_lo += distributed_intensity(a, b, w1, w2, lo);
                        w_hi += distributed_intensity(a, b, w1, w2, hi);
                    }
                    _ => {}
                }
            }

            // Mzの微分はQy。荷重境界ではせん断が不連続なので区間内側の符号を用いる。
            let q1 = width * w_lo;
            let q2 = width * (w_hi * 0.5 - w_lo * 0.5);
            if ![q0, q1, q2].into_iter().all(f64::is_finite) {
                return Err("強軸曲げ区間のせん断力を有限値として確認できません".into());
            }
            let scale = q0.abs().max(q1.abs()).max(q2.abs());
            if scale == 0.0 {
                record_candidate(lo.midpoint(hi), true)?;
                if lo < demand_hi
                    && demand_lo < hi
                    && !response.constant_plateau_axial_and_weak_bending(lo, hi)?
                {
                    let abs_mz_nmm = response.at_mm(lo.midpoint(hi))?[5].abs();
                    unverified_flat_maximum_nmm =
                        Some(unverified_flat_maximum_nmm.unwrap_or(0.0).max(abs_mz_nmm));
                }
                lo = hi;
                continue;
            }
            let (c, b, a) = (q0 / scale, q1 / scale, q2 / scale);
            if [(q0, c), (q1, b), (q2, a)]
                .into_iter()
                .any(|(value, normalized)| value != 0.0 && normalized == 0.0)
            {
                return Err("強軸曲げ区間のせん断力の符号を確認できません".into());
            }
            let vertex = -0.5 * b / a;
            for t in [
                Some(0.0),
                Some(1.0),
                (0.0 < vertex && vertex < 1.0).then_some(vertex),
            ]
            .into_iter()
            .flatten()
            {
                let q = (a * t).mul_add(t, b.mul_add(t, c));
                has_positive_shear |= q > 0.0;
                has_negative_shear |= q < 0.0;
                let magnitude = (a * t * t).abs() + (b * t).abs() + c.abs();
                let confirmed_zero = t == 1.0
                    && q == 0.0
                    && hi == span_hi
                    && end[1] == 0.0
                    && (include_ends || self.observed_shear_y(hi).is_none_or(|left| left == 0.0));
                uncertain_shear_sign |= !confirmed_zero
                    && magnitude > 0.0
                    && q.abs() <= 64.0 * f64::EPSILON * magnitude;
            }
            for t in quadratic_roots(a, b, c)
                .into_iter()
                .flatten()
                .filter(|&t| 0.0 < t && t < 1.0)
            {
                record_candidate(t.mul_add(width, lo), false)?;
            }
            lo = hi;
        }

        let maximum = candidates
            .iter()
            .map(|(_, force)| force[5].abs())
            .fold(0.0, f64::max);
        let tolerance = 64.0 * f64::EPSILON * maximum;
        candidates.sort_by(|(x, _), (y, _)| x.total_cmp(y));
        candidates.dedup_by(|(x, _), (y, _)| *x == *y);
        let demand_peaks = candidates
            .into_iter()
            .filter(|(x, force)| {
                maximum - force[5].abs() <= tolerance
                    || include_ends && (*x == demand_lo || *x == demand_hi)
            })
            .map(|(x, left)| {
                let right = if x < demand_hi {
                    let jump = response.point_load_jump(x);
                    if jump == [0.0; 3] {
                        Ok(None)
                    } else {
                        let mut right = left;
                        for (force, jump) in right.iter_mut().zip(jump) {
                            *force += jump;
                        }
                        if right.iter().all(|value| value.is_finite()) {
                            Ok(Some(right))
                        } else {
                            Err("集中荷重の右側の断面力が非有限です".into())
                        }
                    }
                } else {
                    Ok(None)
                };
                FrameStrongBendingPeak {
                    xi: x / self.length_mm,
                    x_mm: x,
                    left,
                    right,
                    is_i_face: x == demand_lo,
                    is_j_face: x == demand_hi,
                }
            })
            .collect();
        Ok(FrameStrongBendingSpan {
            end_moments_nmm: [start[5], end[5]],
            monotonic: !(boundary_shear_conflicts
                || uncertain_shear_sign
                || has_positive_shear && has_negative_shear),
            interior_maximum: !boundary_shear_conflicts && interior_maximum,
            unverified_maximum_plateau: maximum > 0.0
                && unverified_flat_maximum_nmm.is_some_and(|mz| maximum - mz <= tolerance),
            unverified_distribution: None,
            demand_peaks,
        })
    }

    fn station_demands_without_distribution(&self, reason: &'static str) -> FrameStrongBendingSpan {
        let demand_peaks = self
            .stations_mm
            .iter()
            .max_by(|(_, a), (_, b)| a[5].abs().total_cmp(&b[5].abs()))
            .map(|&(x_mm, left)| FrameStrongBendingPeak {
                xi: x_mm / self.length_mm,
                x_mm,
                left,
                right: Ok(None),
                is_i_face: x_mm == 0.0,
                is_j_face: x_mm == self.length_mm,
            })
            .into_iter()
            .collect();
        FrameStrongBendingSpan {
            end_moments_nmm: [
                self.stations_mm[0].1[5],
                self.stations_mm[self.stations_mm.len() - 1].1[5],
            ],
            monotonic: false,
            interior_maximum: false,
            unverified_maximum_plateau: false,
            unverified_distribution: Some(reason),
            demand_peaks,
        }
    }
}

/// 線形静的応答から、i端起点の実距離 [mm] と局所断面力 [N, N·mm] を照合する。
/// 両端必須。位置は昇順かつ重複なしとし、集中荷重と同位置では左極限の断面力を照合する。
pub fn verify_frame_bending_stations_mm(
    frame: &LocalFrame,
    length_mm: f64,
    loads: &[(f64, &MemberLoad)],
    stations_mm: &[(f64, [f64; 6])],
) -> Result<(), String> {
    verified_frame_bending_response_mm(frame, length_mm, loads, stations_mm).map(|_| ())
}

fn verified_frame_bending_response_mm(
    frame: &LocalFrame,
    length_mm: f64,
    loads: &[(f64, &MemberLoad)],
    stations_mm: &[(f64, [f64; 6])],
) -> Result<LoadedFrameSection, String> {
    let response = frame_bending_response_mm(frame, length_mm, loads, stations_mm)?;
    verify_bending_stations(&response, stations_mm)?;
    Ok(response)
}

fn frame_bending_response_mm(
    frame: &LocalFrame,
    length_mm: f64,
    loads: &[(f64, &MemberLoad)],
    stations_mm: &[(f64, [f64; 6])],
) -> Result<LoadedFrameSection, String> {
    let Some(&(0.0, start)) = stations_mm.first() else {
        return Err("i端の解析断面力がありません".into());
    };
    let Some(&(distance_mm, _)) = stations_mm.last() else {
        return Err("j端の解析断面力がありません".into());
    };
    if distance_mm != length_mm {
        return Err("j端の解析断面力がありません".into());
    }
    if stations_mm
        .iter()
        .any(|(x_mm, _)| !(0.0..=length_mm).contains(x_mm))
        || stations_mm.windows(2).any(|pair| pair[0].0 >= pair[1].0)
    {
        return Err("解析断面力の評価位置が不正です".into());
    }
    let loads = loads
        .iter()
        .map(|&(factor, load)| {
            let mut load = load.clone();
            match &mut load.kind {
                MemberLoadKind::Point { p, .. } => *p *= factor,
                MemberLoadKind::Distributed { w1, w2, .. } => {
                    *w1 *= factor;
                    *w2 *= factor;
                }
            }
            load
        })
        .collect::<Vec<_>>();
    let map = FrameDeadLoadMap::new(&loads, frame, length_mm, 0.0, 0.0)?;
    let origin = [
        -start[0], start[1], start[2], -start[3], -start[4], -start[5],
    ];
    let mut node_force = [0.0; 12];
    node_force[..6].copy_from_slice(&origin);
    Ok(LoadedFrameSection::new(map, node_force))
}

fn verify_bending_stations(
    response: &LoadedFrameSection,
    stations_mm: &[(f64, [f64; 6])],
) -> Result<(), String> {
    verify_bending_stations_with(stations_mm, |x_mm| response.at_mm(x_mm))
}

fn verify_bending_stations_with(
    stations_mm: &[(f64, [f64; 6])],
    recover: impl Fn(f64) -> Result<[f64; 6], String>,
) -> Result<(), String> {
    let scales: [f64; 6] = std::array::from_fn(|i| {
        stations_mm
            .iter()
            .map(|(_, f)| f[i].abs())
            .fold(1.0, f64::max)
    });
    for &(x_mm, observed) in stations_mm {
        let recovered = recover(x_mm)?;
        if (0..6).any(|i| {
            !observed[i].is_finite()
                || (recovered[i] - observed[i]).abs() > 1e-8 * scales[i].max(recovered[i].abs())
        }) {
            return Err("解析断面力と部材荷重の釣合いを確認できません".into());
        }
    }
    Ok(())
}

impl LoadedFrameSection {
    /// 同じ解析応答の全長で符号付き強軸曲げの分布と最大需要を返す。
    /// 指定した実材軸長 [mm] や記録断面力と対応しない場合は理由付きエラー。
    pub fn full_span_strong_bending(
        &self,
        length_mm: f64,
        forces: &MemberForces,
    ) -> Result<FrameStrongBendingSpan, String> {
        let response_length = self.map.length_mm;
        let tolerance_mm = 32.0 * f64::EPSILON * response_length.max(1.0);
        if !length_mm.is_finite() || (length_mm - response_length).abs() > tolerance_mm {
            return Err("実材軸長が部材荷重応答の全長と一致しません".into());
        }
        let stations = bending_stations_mm(response_length, forces)?;
        if stations
            .iter()
            .any(|(x, _)| !(0.0..=response_length).contains(x))
            || stations.windows(2).any(|pair| pair[0].0 >= pair[1].0)
        {
            return Err("解析断面力の評価位置が不正です".into());
        }
        verify_bending_stations(self, &stations)?;
        let response =
            StrongBendingResponse::new(self.map.frame(), response_length, self, &stations);
        match response.exact_full_span() {
            Ok(bending) => Ok(bending),
            Err(ExactSpanError::VaryingStrongLoad) => Ok(response
                .station_demands_without_distribution(
                    "強軸方向の合算分布荷重の強度が区間内で変化するため、曲げ分布を確定できません",
                )),
            Err(ExactSpanError::Unconfirmed) => Ok(response.station_demands_without_distribution(
                "強軸曲げの分布と最大位置の断面力を有限値として確定できません",
            )),
        }
    }

    fn constant_plateau_axial_and_weak_bending(
        &self,
        lo_mm: f64,
        hi_mm: f64,
    ) -> Result<bool, String> {
        let shear_z_n = self.at_mm(lo_mm)?[2] + self.point_load_jump(lo_mm)[2];
        Ok(shear_z_n == 0.0 && !self.has_axial_or_weak_distributed_load(lo_mm, hi_mm))
    }

    fn has_axial_or_weak_distributed_load(&self, lo_mm: f64, hi_mm: f64) -> bool {
        self.map.loads.iter().any(|load| {
            let [axial, _, weak] = resolve(load, self.map.frame());
            [axial, weak].into_iter().flatten().any(|component| {
                matches!(component, Comp::Dist { a, b, w1, w2 }
                    if a < hi_mm && lo_mm < b && (w1 != 0.0 || w2 != 0.0))
            })
        })
    }

    pub(super) fn point_load_jump(&self, x_mm: f64) -> [f64; 3] {
        let mut jump = [0.0; 3];
        for load in &self.map.loads {
            if matches!(load.kind, MemberLoadKind::Point { a, .. } if a == x_mm) {
                for (i, comp) in resolve(load, self.map.frame()).into_iter().enumerate() {
                    if let Some(Comp::Point { p, .. }) = comp {
                        jump[i] += if i == 0 { -p } else { p };
                    }
                }
            }
        }
        jump
    }
}

pub(super) fn quadratic_roots(a: f64, b: f64, c: f64) -> [Option<f64>; 2] {
    if a == 0.0 {
        return [(b != 0.0).then(|| -c / b), None];
    }
    let discriminant = b.mul_add(b, -4.0 * a * c);
    if discriminant < 0.0 {
        return [None, None];
    }
    let q = -0.5 * (b + discriminant.sqrt().copysign(b));
    if q == 0.0 {
        [Some(-0.5 * b / a), None]
    } else {
        [Some(q / a), Some(c / q)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sepika_core::ids::ElemId;

    #[test]
    fn time_history_confirms_an_exact_off_center_maximum_tied_with_an_end() {
        let length_mm = 3000.0;
        let frame =
            LocalFrame::from_nodes([0.0; 3], [length_mm, 0.0, 0.0], [0.0, 1.0, 0.0]).unwrap();
        let point =
            |a, p| MemberLoad::manual(ElemId(0), [0.0, 1.0, 0.0], MemberLoadKind::Point { a, p });
        for offset_n in [0.0, -1e-9, 1e-9] {
            let loads = [point(1000.0, 500e3 + offset_n), point(2000.0, -500e3)];
            let map = FrameDeadLoadMap::new(&loads, &frame, length_mm, 0.0, 0.0).unwrap();
            let mut node_force = [0.0; 12];
            node_force[1] = -250e3;
            node_force[5] = -250e6;
            let response = LoadedFrameSection::new(map, node_force);
            let forces = response.recover(&[0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0]).unwrap();
            if offset_n == 0.0 {
                assert_eq!(
                    forces
                        .at
                        .iter()
                        .map(|(_, force)| force[5])
                        .collect::<Vec<_>>(),
                    [250e6, 0.0, 250e6, 0.0]
                );
            }

            let actual = response
                .full_span_strong_bending(length_mm, &forces)
                .unwrap();
            assert!(!actual.monotonic);
            assert_eq!(actual.interior_maximum, offset_n >= 0.0);
            if offset_n >= 0.0 {
                let interior = actual
                    .demand_peaks
                    .iter()
                    .find(|peak| peak.x_mm == 2000.0)
                    .unwrap();
                assert_eq!(interior.left[5].total_cmp(&250e6), offset_n.total_cmp(&0.0));
            } else {
                assert!(actual.demand_peaks.iter().all(|peak| peak.is_i_face));
            }
            let static_span = frame_strong_bending_span(
                &frame,
                length_mm,
                [0.0, length_mm],
                &[(1.0, &loads[0]), (1.0, &loads[1])],
                &forces,
            )
            .unwrap();
            assert!(!static_span.interior_maximum);
        }
    }

    #[test]
    fn time_history_confirms_a_uniform_load_maximum_tied_with_an_end() {
        let length_mm = 3000.0;
        let frame =
            LocalFrame::from_nodes([0.0; 3], [length_mm, 0.0, 0.0], [0.0, 1.0, 0.0]).unwrap();
        let distributed = |a, b, w1, w2| {
            MemberLoad::manual(
                ElemId(0),
                [0.0, 1.0, 0.0],
                MemberLoadKind::Distributed { a, b, w1, w2 },
            )
        };
        let uniform = [
            distributed(0.0, 500.0, -400.0, -400.0),
            distributed(500.0, 1500.0, 400.0, 400.0),
            distributed(1500.0, 2500.0, -400.0, -400.0),
        ];
        let cancelled_gradient = [
            distributed(0.0, 500.0, -400.0, -400.0),
            distributed(500.0, 1500.0, 400.0, 400.0),
            distributed(1500.0, 2500.0, -100.0, -250.0),
            distributed(1500.0, 2500.0, -300.0, -150.0),
        ];
        for loads in [uniform.as_slice(), cancelled_gradient.as_slice()] {
            let map = FrameDeadLoadMap::new(loads, &frame, length_mm, 0.0, 0.0).unwrap();
            let mut node_force = [0.0; 12];
            node_force[5] = -250e6;
            let response = LoadedFrameSection::new(map, node_force);
            let forces = response
                .recover(&[0.0, 1.0 / 6.0, 1.0 / 3.0, 0.5, 2.0 / 3.0, 5.0 / 6.0, 1.0])
                .unwrap();
            for ((_, force), expected_nmm) in forces
                .at
                .iter()
                .zip([250e6, 200e6, 150e6, 200e6, 250e6, 200e6, 100e6])
            {
                assert!((force[5] - expected_nmm).abs() < 1e-6);
            }
            let span = response
                .full_span_strong_bending(length_mm, &forces)
                .unwrap();
            assert!(span.demand_peaks.iter().any(|peak| {
                (peak.x_mm - 2000.0).abs() < 1e-9 && (peak.left[5] - 250e6).abs() < 1e-6
            }));
            assert!(!span.monotonic);
            assert!(span.interior_maximum && span.unverified_distribution.is_none());
            let terms = loads.iter().map(|load| (1.0, load)).collect::<Vec<_>>();
            let static_span =
                frame_strong_bending_span(&frame, length_mm, [0.0, length_mm], &terms, &forces)
                    .unwrap();
            assert!(!static_span.interior_maximum);
        }
    }

    #[test]
    fn varying_strong_load_does_not_use_a_rounded_root_to_confirm_zero_weak_bending() {
        let length_mm = 3000.0;
        let frame =
            LocalFrame::from_nodes([0.0; 3], [length_mm, 0.0, 0.0], [0.0, 1.0, 0.0]).unwrap();
        let loads = [
            MemberLoad::manual(
                ElemId(0),
                [0.0, 1.0, 0.0],
                MemberLoadKind::Distributed {
                    a: 0.0,
                    b: length_mm,
                    w1: -40.0,
                    w2: (-40.0_f64).next_up(),
                },
            ),
            MemberLoad::manual(
                ElemId(0),
                [0.0, 0.0, 1.0],
                MemberLoadKind::Point {
                    a: 1500.0,
                    p: 100.0,
                },
            ),
        ];
        for m_i_nmm in [155e6, 245e6] {
            let map = FrameDeadLoadMap::new(&loads, &frame, length_mm, 0.0, 0.0).unwrap();
            let mut node_force = [0.0; 12];
            node_force[..6].copy_from_slice(&[0.0, 60e3, 1e3, 0.0, -1.5e6, -m_i_nmm]);
            let response = LoadedFrameSection::new(map, node_force);
            let forces = response.recover(&[0.0, 0.5, 1.0]).unwrap();
            assert_eq!(forces.at[1].1[4], 0.0);
            let span = response
                .full_span_strong_bending(length_mm, &forces)
                .unwrap();
            assert!(!span.monotonic && !span.interior_maximum);
            assert!(span
                .unverified_distribution
                .is_some_and(|reason| reason.contains("合算分布荷重の強度が区間内で変化")));
            assert_eq!(span.demand_peaks.len(), 1);
            assert_eq!(span.demand_peaks[0].x_mm, 1500.0);
            assert!((span.demand_peaks[0].left[5] - (m_i_nmm + 45e6)).abs() < 1e-6);
        }
    }

    #[test]
    fn time_history_does_not_turn_interior_roots_into_endpoint_maxima() {
        let length_mm = 3000.0;
        let frame =
            LocalFrame::from_nodes([0.0; 3], [length_mm, 0.0, 0.0], [0.0, 1.0, 0.0]).unwrap();
        let point = |a, p, dir| MemberLoad::manual(ElemId(0), dir, MemberLoadKind::Point { a, p });
        let distributed = |w| {
            MemberLoad::manual(
                ElemId(0),
                [0.0, 1.0, 0.0],
                MemberLoadKind::Distributed {
                    a: 0.0,
                    b: length_mm,
                    w1: w,
                    w2: w,
                },
            )
        };
        for (initial_mz_nmm, initial_q_n, loads, displayed_x_mm, expected_n_n) in [
            (
                70e6,
                120_000.000_000_000_01,
                vec![distributed(-40.000_000_000_000_01)],
                3000.0,
                0.0,
            ),
            (
                250e6,
                120_000.0,
                vec![
                    point(0.0, -120_000.0, [0.0, 1.0, 0.0]),
                    point(0.0, 1e-12, [0.0, 1.0, 0.0]),
                    distributed(-40.0),
                ],
                2.5e-14,
                0.0,
            ),
            (
                205e6,
                60_000.0,
                vec![
                    point(1500.0, 1e-12, [0.0, 1.0, 0.0]),
                    point(1500.0, -100.0, [1.0, 0.0, 0.0]),
                    distributed(-40.0),
                ],
                1500.0,
                100.0,
            ),
        ] {
            let map = FrameDeadLoadMap::new(&loads, &frame, length_mm, 0.0, 0.0).unwrap();
            let mut node_force = [0.0; 12];
            node_force[1] = initial_q_n;
            node_force[5] = -initial_mz_nmm;
            let response = LoadedFrameSection::new(map, node_force);
            let forces = response.recover(&[0.0, 0.5, 1.0]).unwrap();
            let bending = response
                .full_span_strong_bending(length_mm, &forces)
                .unwrap();
            assert!(bending.interior_maximum && !bending.monotonic);
            assert_eq!(bending.demand_peaks.len(), 1);
            let peak = &bending.demand_peaks[0];
            assert!(!peak.is_i_face && !peak.is_j_face);
            assert!((peak.x_mm - displayed_x_mm).abs() < 1e-28);
            assert_eq!(peak.left[0], expected_n_n);
            assert_eq!(peak.left[1], 0.0);
            assert!((peak.left[5] - 250e6).abs() < 1e-6);
            assert!(matches!(peak.right, Ok(None)));
        }
    }

    #[test]
    fn cancellation_order_cannot_promote_or_hide_an_exact_time_history_maximum() {
        let length_mm = 3000.0;
        let frame =
            LocalFrame::from_nodes([0.0; 3], [length_mm, 0.0, 0.0], [0.0, 1.0, 0.0]).unwrap();
        let distributed = |a, b, w| {
            MemberLoad::manual(
                ElemId(0),
                [0.0, 1.0, 0.0],
                MemberLoadKind::Distributed { a, b, w1: w, w2: w },
            )
        };
        let s = 500.062_500_953_674_3;
        let i = distributed(0.0, s, -400.0);
        let center = distributed(s, 3.0 * s, 400.0);
        let j = distributed(3.0 * s, 5.0 * s, -400.0);
        let plus = distributed(0.0, length_mm, 40_000.0);
        let minus = distributed(0.0, length_mm, -40_000.0);
        let j_unequal = distributed(3.0 * s, 5.0 * s, (-400.0_f64).next_down());
        for (loads, internal) in [
            ([&minus, &center, &i, &j, &plus], true),
            ([&j, &i, &plus, &minus, &center], true),
            ([&j_unequal, &i, &plus, &minus, &center], false),
        ] {
            let loads = loads.map(Clone::clone);
            let map = FrameDeadLoadMap::new(&loads, &frame, length_mm, 0.0, 0.0).unwrap();
            let mut node_force = [0.0; 12];
            node_force[5] = -250e6;
            let response = LoadedFrameSection::new(map, node_force);
            let forces = response.recover(&[0.0, 1.0]).unwrap();
            let span = response
                .full_span_strong_bending(length_mm, &forces)
                .unwrap();
            assert!(!span.monotonic);
            assert_eq!(span.interior_maximum, internal);
            assert_eq!(span.demand_peaks.len(), if internal { 2 } else { 1 });
            assert_eq!(span.demand_peaks[0].x_mm, 0.0);
            assert_eq!(span.demand_peaks[0].left[5], 250e6);
            if internal {
                assert_eq!(span.demand_peaks[1].x_mm, 4.0 * s);
                assert_eq!(span.demand_peaks[1].left[5], 250e6);
            }
        }
    }

    #[test]
    fn rounded_endpoint_ties_do_not_prove_interior_maxima_in_time_history() {
        let length_mm = 3000.0;
        let frame =
            LocalFrame::from_nodes([0.0; 3], [length_mm, 0.0, 0.0], [0.0, 1.0, 0.0]).unwrap();
        let point =
            |a, p| MemberLoad::manual(ElemId(0), [0.0, 1.0, 0.0], MemberLoadKind::Point { a, p });
        for (p, internal) in [
            (20_000.0_f64.next_down(), false),
            (20_000.0_f64.next_up(), true),
        ] {
            let loads = [point(500.0, p), point(1000.0, -20e3)];
            let map = FrameDeadLoadMap::new(&loads, &frame, length_mm, 0.0, 0.0).unwrap();
            let mut node_force = [0.0; 12];
            node_force[1] = -10e3;
            node_force[5] = -250e6;
            let response = LoadedFrameSection::new(map, node_force);
            let forces = response.recover(&[0.0, 1.0]).unwrap();
            let span = response
                .full_span_strong_bending(length_mm, &forces)
                .unwrap();
            assert_eq!(span.end_moments_nmm[0], 250e6);
            assert_eq!(response.at_mm(1000.0).unwrap()[5], 250e6);
            assert!(!span.monotonic);
            assert_eq!(span.interior_maximum, internal);
            assert_eq!(span.demand_peaks.len(), 1);
            assert_eq!(
                span.demand_peaks[0].x_mm,
                if internal { 1000.0 } else { 0.0 }
            );
            assert_eq!(span.demand_peaks[0].left[5], 250e6);
        }
    }

    #[test]
    fn time_history_confirms_exact_plateaux_but_rejects_rounded_shear_cancellation() {
        let length_mm = 3000.0;
        let frame =
            LocalFrame::from_nodes([0.0; 3], [length_mm, 0.0, 0.0], [0.0, 1.0, 0.0]).unwrap();
        let point =
            |a, p| MemberLoad::manual(ElemId(0), [0.0, 1.0, 0.0], MemberLoadKind::Point { a, p });
        for (loads, initial_qy_n, confirmed) in [
            (vec![], 0.0, true),
            (vec![point(500.0, 20e3), point(1000.0, -10e3)], -10e3, true),
            (
                vec![
                    point(500.0, 20e3),
                    point(750.0, -1e-12),
                    point(1000.0, -10e3),
                ],
                -10e3,
                false,
            ),
            (
                vec![
                    point(500.0, 20e3),
                    point(750.0, 1e-12),
                    point(1000.0, -10e3),
                ],
                -10e3,
                false,
            ),
        ] {
            let map = FrameDeadLoadMap::new(&loads, &frame, length_mm, 0.0, 0.0).unwrap();
            let mut node_force = [0.0; 12];
            node_force[1] = initial_qy_n;
            node_force[5] = -250e6;
            let response = LoadedFrameSection::new(map, node_force);
            let forces = response.recover(&[0.0, 1.0]).unwrap();
            let span = response
                .full_span_strong_bending(length_mm, &forces)
                .unwrap();
            assert_eq!(span.interior_maximum, confirmed, "{loads:?}");
            assert_eq!(
                span.demand_peaks
                    .iter()
                    .any(|peak| { !peak.is_i_face && !peak.is_j_face && peak.left[5] == 250e6 }),
                confirmed
            );
        }
    }

    #[test]
    fn a_verified_terminal_zero_does_not_hide_a_nonzero_shear_reversal() {
        let frame = LocalFrame::from_nodes([0.0; 3], [8_000.0, 0.0, 0.0], [0.0, 1.0, 0.0]).unwrap();
        let load = MemberLoad::manual(
            ElemId(0),
            [0.0, 1.0, 0.0],
            MemberLoadKind::Distributed {
                a: 0.0,
                b: 8_000.0,
                w1: -1.0,
                w2: -1.0,
            },
        );
        let start = [-32e6, -8e6, 0.0];
        for (delta_q, monotonic) in [(0.0, true), (-1e-11, false)] {
            let forces = MemberForces {
                at: [0.0, 0.5, 1.0]
                    .into_iter()
                    .zip(start)
                    .map(|(xi, m)| {
                        (
                            xi,
                            [
                                0.0,
                                8_000.0 * (1.0 - xi) + delta_q,
                                0.0,
                                0.0,
                                0.0,
                                m + delta_q * xi * 8_000.0,
                            ],
                        )
                    })
                    .collect(),
            };
            let span = frame_strong_bending_span(
                &frame,
                8_000.0,
                [0.0, 8_000.0],
                &[(1.0, &load)],
                &forces,
            )
            .unwrap();
            assert_eq!(span.monotonic, monotonic);
            assert!(!span.interior_maximum);
            assert_eq!(span.demand_peaks.len(), 1);
            assert_eq!(span.demand_peaks[0].xi, 0.0);
        }
    }
}
