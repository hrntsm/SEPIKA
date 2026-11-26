use super::{distributed_intensity, resolve, Comp, FrameDeadLoadMap, LoadedFrameSection};
use crate::frame::beam::MemberForces;
use crate::transform::LocalFrame;
use sepika_core::model::{MemberLoad, MemberLoadKind};

/// 補剛区間の符号付き強軸曲げ分布と、指定した需要範囲の断面力。
pub struct FrameStrongBendingSpan {
    /// 補剛区間のi/j側端の曲げモーメント [N·mm]。
    pub end_moments_nmm: [f64; 2],
    /// 符号付き曲げが補剛区間で非減少または非増加と確認できた場合にtrue。
    pub monotonic: bool,
    /// 区間内部で端部以上の曲げ絶対値を確認できた場合にtrue。
    pub interior_maximum: bool,
    /// 需要範囲で絶対値最大に達する断面。複数区間の照査では需要範囲の両端も含む。
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
            demand_peaks,
        })
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
    let response = LoadedFrameSection::new(map, node_force);
    let scales: [f64; 6] = std::array::from_fn(|i| {
        stations_mm
            .iter()
            .map(|(_, f)| f[i].abs())
            .fold(1.0, f64::max)
    });
    for &(x_mm, observed) in stations_mm {
        let recovered = response.at_mm(x_mm)?;
        if (0..6).any(|i| {
            !observed[i].is_finite()
                || (recovered[i] - observed[i]).abs() > 1e-8 * scales[i].max(recovered[i].abs())
        }) {
            return Err("解析断面力と部材荷重の釣合いを確認できません".into());
        }
    }

    Ok(response)
}

impl LoadedFrameSection {
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
