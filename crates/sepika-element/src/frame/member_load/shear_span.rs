use super::{resolve, Comp, LoadedFrameSection};
use sepika_core::model::MemberLoadKind;

/// 断面力を三次 Bernstein 形式で保持する。制御値の範囲は区間内の断面力を包含する。
pub(in crate::frame) struct ShearSpan([[f64; 6]; 4]);

impl ShearSpan {
    pub(in crate::frame) fn bounds(&self) -> ([f64; 2], [f64; 2], [[f64; 2]; 2]) {
        let max_abs = |i: usize| {
            self.0
                .iter()
                .map(|point| point[i].abs())
                .fold(0.0, f64::max)
        };
        let bounds = |i: usize| {
            self.0
                .iter()
                .map(|point| point[i])
                .fold([f64::INFINITY, f64::NEG_INFINITY], |[lo, hi], value| {
                    [lo.min(value), hi.max(value)]
                })
        };
        let [axial_lo, axial_hi] = bounds(0);
        (
            [max_abs(1), max_abs(2)],
            [-axial_hi, -axial_lo],
            [bounds(5), bounds(4)],
        )
    }

    pub(in crate::frame) fn bisect(&self) -> [Self; 2] {
        let [a, b, c, d] = self.0;
        let mid = |x: [f64; 6], y: [f64; 6]| std::array::from_fn(|i| x[i].midpoint(y[i]));
        let [e, f, g] = [mid(a, b), mid(b, c), mid(c, d)];
        let [h, i] = [mid(e, f), mid(f, g)];
        let m = mid(h, i);
        [Self([a, e, h, m]), Self([m, i, g, d])]
    }
}

impl LoadedFrameSection {
    /// 可撓面と同じ位置の集中荷重 [N]。局所軸、i/j端の順。位置が区別不能なら理由を返す。
    pub(in crate::frame) fn flex_face_point_loads(&self) -> [Result<[f64; 3], String>; 2] {
        let [left, right] = [self.map.li_mm, self.map.length_mm - self.map.lj_mm];
        let tol_mm = 32.0 * f64::EPSILON * self.map.length_mm.max(1.0);
        [left, right].map(|face| {
            let mut force = [0.0; 3];
            for load in &self.map.loads {
                let MemberLoadKind::Point { a, p } = load.kind else {
                    continue;
                };
                if p == 0.0 || (a - face).abs() > tol_mm {
                    continue;
                }
                if a != face {
                    return Err(
                        "可撓面と近接した集中荷重の位置関係を丸め誤差から区別できません".into(),
                    );
                }
                for (i, comp) in resolve(load, &self.map.frame).into_iter().enumerate() {
                    if let Some(Comp::Point { p, .. }) = comp {
                        force[i] += p;
                    }
                }
            }
            Ok(force)
        })
    }

    pub(in crate::frame) fn flex_shear_spans(&self) -> Result<Vec<ShearSpan>, String> {
        let [left, right] = [self.map.li_mm, self.map.length_mm - self.map.lj_mm];
        let mut breaks = vec![left, right];
        for load in &self.map.loads {
            let edges = match load.kind {
                MemberLoadKind::Point { a, .. } => [a, a],
                MemberLoadKind::Distributed { a, b, .. } => [a, b],
            };
            breaks.extend(edges.into_iter().filter(|&x| left < x && x < right));
        }
        breaks.sort_by(f64::total_cmp);
        breaks.dedup();
        breaks
            .windows(2)
            .map(|range| {
                let a = range[0];
                let b = range[1];
                let mid = a.midpoint(b);
                if !(a < mid && mid < b) {
                    return Err("可撓区間の近接した荷重境界を丸め誤差から区別できません".into());
                }
                let mut start = self.at_mm(a)?;
                for load in &self.map.loads {
                    if !matches!(load.kind, MemberLoadKind::Point { a: x, .. } if x == a) {
                        continue;
                    }
                    for (i, comp) in resolve(load, &self.map.frame).into_iter().enumerate() {
                        if let Some(Comp::Point { p, .. }) = comp {
                            start[i] += if i == 0 { -p } else { p };
                        }
                    }
                }
                let end = self.at_mm(b)?;
                let half = self.at_mm(mid)?;
                let length_mm = b - a;
                let control = [
                    start,
                    std::array::from_fn(|i| match i {
                        // M'y=-Qz, M'z=Qy. 端のモーメントと微分から三次項を定める。
                        4 => start[4] - length_mm * start[2] / 3.0,
                        5 => start[5] + length_mm * start[1] / 3.0,
                        _ => (4.0 * half[i] - end[i]) / 3.0,
                    }),
                    std::array::from_fn(|i| match i {
                        4 => end[4] + length_mm * end[2] / 3.0,
                        5 => end[5] - length_mm * end[1] / 3.0,
                        _ => (4.0 * half[i] - start[i]) / 3.0,
                    }),
                    end,
                ];
                if !control.iter().flatten().all(|v| v.is_finite()) {
                    return Err("可撓区間のせん断応答を有限値として確認できません".into());
                }
                Ok(ShearSpan(control))
            })
            .collect()
    }
}
