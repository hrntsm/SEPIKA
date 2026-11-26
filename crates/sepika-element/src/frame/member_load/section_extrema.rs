use super::{distributed_intensity, resolve, Comp, LoadedFrameSection, MemberLoadKind};
use crate::transform::LocalFrame;

impl LoadedFrameSection {
    /// 部材荷重を回復する全長軸の局所基底。
    pub fn frame(&self) -> &LocalFrame {
        self.map.frame()
    }

    /// 全長に作用する非零の部材荷重に対し、回復でゼロに置換された局所成分がないことを確認する。
    /// 成分が物理的にゼロと確認できなければエラー。
    pub fn verify_load_projections(&self) -> Result<(), String> {
        self.verify_load_projections_with(|dir, index| {
            dir.into_iter()
                .zip(self.frame().rot[index])
                .all(|(d, a)| d == 0.0 || a == 0.0)
        })
    }

    pub(super) fn verify_load_projections_with(
        &self,
        provably_zero: impl Fn([f64; 3], usize) -> bool,
    ) -> Result<(), String> {
        for load in &self.map.loads {
            let specified = match load.kind {
                MemberLoadKind::Point { p, .. } => [p, 0.0],
                MemberLoadKind::Distributed { w1, w2, .. } => [w1, w2],
            };
            if specified == [0.0; 2] {
                continue;
            }
            for (index, comp) in resolve(load, self.frame()).into_iter().enumerate() {
                let recovered = match comp {
                    Some(Comp::Point { p, .. }) => [p, 0.0],
                    Some(Comp::Dist { w1, w2, .. }) => [w1, w2],
                    None => [0.0; 2],
                };
                if specified
                    .into_iter()
                    .zip(recovered)
                    .any(|(input, output)| input != 0.0 && output == 0.0)
                    && !provably_zero(load.dir, index)
                {
                    return Err(
                        "部材荷重の局所軸成分が回復時に保持されることを確認できません".into(),
                    );
                }
            }
        }
        Ok(())
    }

    /// 実材軸i端からの距離と実材軸長［mm］を指定し、区間内の各断面力の最小・最大を返す。
    /// 成分順は N, Qy, Qz, Mx, My, Mz［N, N·mm］。内点の集中荷重は左右、外端は区間内側を用いる。
    /// 回復座標・荷重境界・断面力を確認できないときはエラー。
    pub fn section_force_ranges_mm(
        &self,
        [lo, hi]: [f64; 2],
        length_mm: f64,
    ) -> Result<[[f64; 2]; 6], String> {
        let stored_mm = self.map.length_mm();
        let tol_mm = 32.0 * f64::EPSILON * stored_mm.max(1.0);
        if !length_mm.is_finite()
            || (stored_mm - length_mm).abs() > tol_mm
            || !(0.0 <= lo && lo < hi && hi <= length_mm)
        {
            return Err("実材軸長または断面力の評価区間が部材荷重応答の全長と一致しません".into());
        }
        let hi = if hi == length_mm { stored_mm } else { hi };
        if !(lo < hi && hi <= stored_mm) {
            return Err("断面力の評価区間が部材荷重応答の全長区間外です".into());
        }
        self.verify_load_projections()?;

        let mut breaks = vec![lo, hi];
        let mut distributed = [vec![], vec![], vec![]];
        for load in &self.map.loads {
            let edges = match load.kind {
                MemberLoadKind::Point { a, p } if p != 0.0 => {
                    if [lo, hi]
                        .into_iter()
                        .any(|x| a != x && (a - x).abs() <= tol_mm)
                    {
                        return Err("評価区間端と近接した集中荷重の位置関係を区別できません".into());
                    }
                    [a, a]
                }
                MemberLoadKind::Distributed { a, b, w1, w2 } if w1 != 0.0 || w2 != 0.0 => {
                    for (spans, comp) in distributed.iter_mut().zip(resolve(load, self.frame())) {
                        if let Some(Comp::Dist { a, b, w1, w2 }) = comp {
                            spans.push((a, b, w1, w2));
                        }
                    }
                    [a, b]
                }
                _ => continue,
            };
            breaks.extend(edges.into_iter().filter(|&x| lo < x && x < hi));
        }
        breaks.sort_by(f64::total_cmp);
        breaks.dedup();
        let mut ranges = [[f64::INFINITY, f64::NEG_INFINITY]; 6];
        let mut include = |values: [f64; 6]| {
            if !values.into_iter().all(f64::is_finite) {
                return Err("区間内の断面力が非有限です".to_owned());
            }
            for ([min, max], value) in ranges.iter_mut().zip(values) {
                *min = min.min(value);
                *max = max.max(value);
            }
            Ok(())
        };
        for limits in breaks.windows(2) {
            let a = limits[0];
            let b = limits[1];
            let width_mm = b - a;
            let mut start = self.at_mm(a)?;
            for (value, jump) in start.iter_mut().zip(self.point_load_jump(a)) {
                *value += jump;
            }
            include(start)?;
            include(self.at_mm(b)?)?;
            if a.midpoint(b) == a || a.midpoint(b) == b {
                return Err("近接した荷重境界を丸め誤差から区別できません".into());
            }
            for (axis, spans) in distributed.iter().enumerate() {
                let [w_a, w_b] = spans
                    .iter()
                    .filter(|&&(lo, hi, _, _)| lo <= a && b <= hi)
                    .fold([0.0, 0.0], |[w_a, w_b], &(lo, hi, w1, w2)| {
                        [
                            w_a + distributed_intensity(lo, hi, w1, w2, a),
                            w_b + distributed_intensity(lo, hi, w1, w2, b),
                        ]
                    });
                let linear = width_mm * w_a;
                let quadratic = width_mm * (0.5 * w_b - 0.5 * w_a);
                if ![linear, quadratic].into_iter().all(f64::is_finite) {
                    return Err("荷重区間の断面力の係数が非有限です".into());
                }
                let mut include_at = |t: f64| {
                    let x_mm = t.mul_add(width_mm, a);
                    if 0.0 < t && t < 1.0 && a < x_mm && x_mm < b {
                        include(self.at_mm(x_mm)?)?;
                    }
                    Ok::<_, String>(())
                };
                if quadratic != 0.0 {
                    include_at(-0.5 * linear / quadratic)?;
                }
                if axis != 0 && (linear != 0.0 || quadratic != 0.0) {
                    let coeff = [quadratic, linear, start[axis]];
                    let scale = coeff.into_iter().map(f64::abs).fold(0.0, f64::max);
                    let scaled = coeff.map(|c| c / scale);
                    if coeff
                        .into_iter()
                        .zip(scaled)
                        .any(|(c, s)| c != 0.0 && s == 0.0)
                    {
                        return Err("曲げの極値を決めるせん断力の係数を保持できません".into());
                    }
                    let [q2, q1, q0] = scaled;
                    // 逆符号を含む荷重では、区間端せん断の符号だけで曲げの極値を特定できない。
                    for t in super::strong_bending::quadratic_roots(q2, q1, q0)
                        .into_iter()
                        .flatten()
                    {
                        include_at(t)?;
                    }
                }
            }
        }
        Ok(ranges)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::member_load::FrameDeadLoadMap;
    use sepika_core::ids::ElemId;
    use sepika_core::model::MemberLoad;

    fn section(
        length_mm: f64,
        qy_i_n: f64,
        loads: impl IntoIterator<Item = MemberLoadKind>,
    ) -> LoadedFrameSection {
        let frame =
            LocalFrame::from_nodes([0.0; 3], [length_mm, 0.0, 0.0], [0.0, 1.0, 0.0]).unwrap();
        let loads: Vec<_> = loads
            .into_iter()
            .map(|kind| MemberLoad::manual(ElemId(0), frame.rot[1], kind))
            .collect();
        let map = FrameDeadLoadMap::new(&loads, &frame, length_mm, 0.0, 0.0).unwrap();
        let mut nodal = [0.0; 12];
        nodal[1] = qy_i_n;
        LoadedFrameSection::new(map, nodal)
    }

    #[test]
    fn point_load_at_inner_boundary_retains_two_sides_but_outer_faces_keep_only_the_inside() {
        let response = section(
            1_000.0,
            30.0,
            [
                MemberLoadKind::Point { a: 0.0, p: -40.0 },
                MemberLoadKind::Point { a: 500.0, p: 50.0 },
                MemberLoadKind::Point {
                    a: 1_000.0,
                    p: -100.0,
                },
            ],
        );
        let ranges = response
            .section_force_ranges_mm([0.0, 1_000.0], 1_000.0)
            .unwrap();
        assert_eq!(ranges[1], [-10.0, 40.0]);
        assert_eq!(ranges[5], [-5_000.0, 15_000.0]);
        assert_eq!(
            response
                .section_force_ranges_mm([0.0, 500.0], 1_000.0)
                .unwrap()[1],
            [-10.0; 2]
        );
        assert_eq!(
            response
                .section_force_ranges_mm([500.0, 1_000.0], 1_000.0)
                .unwrap()[1],
            [40.0; 2]
        );
        assert!(response
            .section_force_ranges_mm([500.0 + 1e-12, 1_000.0], 1_000.0)
            .is_err());
    }

    #[test]
    fn signed_trapezoid_recovers_interior_shear_and_both_opposite_bending_extrema() {
        let response = section(
            4_000.0,
            -6_000.0,
            [MemberLoadKind::Distributed {
                a: 0.0,
                b: 4_000.0,
                w1: 8.0,
                w2: -8.0,
            }],
        );
        let ranges = response
            .section_force_ranges_mm([400.0, 3_600.0], 4_000.0)
            .unwrap();
        // Q = -6000 + 8x - x²/500。Qの極大は x=2000、Mの極値は x=1000, 3000 mm。
        for (actual, expected) in ranges[1].into_iter().chain(ranges[5]).zip([
            -3_120.0,
            2_000.0,
            -2_666_666.666_666_667,
            0.0,
        ]) {
            assert!((actual - expected).abs() < 1e-7, "{actual} != {expected}");
        }
    }

    #[test]
    fn force_ranges_do_not_accept_a_discarded_projection_outside_the_evaluation_range() {
        let frame = LocalFrame::from_nodes([0.0; 3], [1_000.0, 0.0, 0.0], [0.0, 1.0, 0.0]).unwrap();
        let load = |dir| {
            MemberLoad::manual(
                ElemId(0),
                dir,
                MemberLoadKind::Distributed {
                    a: 0.0,
                    b: 300.0,
                    w1: 1.0,
                    w2: 1.0,
                },
            )
        };
        let map = |dir| FrameDeadLoadMap::new(&[load(dir)], &frame, 1_000.0, 0.0, 0.0).unwrap();
        let perpendicular = map([0.0, 1.0, 0.0]);
        let near_perpendicular = map([1e-16, 1.0, 0.0]);
        assert_eq!(perpendicular.flex_load(), near_perpendicular.flex_load());
        let range = [400.0, 900.0];
        let regular = LoadedFrameSection::new(perpendicular, [0.0; 12]);
        assert_eq!(
            regular.section_force_ranges_mm(range, 1_000.0).unwrap()[0],
            [0.0; 2]
        );
        let uncertain = LoadedFrameSection::new(near_perpendicular, [0.0; 12]);
        assert!(uncertain
            .section_force_ranges_mm(range, 1_000.0)
            .unwrap_err()
            .contains("局所軸成分"));
    }
}
