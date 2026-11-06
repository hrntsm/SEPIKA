//! 時系列のレインフロー計数。

/// レインフローで抽出した 1 サイクル。`range` は振れ幅（peak-to-peak）、
/// `count` は回数（全サイクル=1.0、半サイクル=0.5）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RainflowCycle {
    pub range: f64,
    pub count: f64,
}

/// 時系列を折返し点（peaks/valleys）の列へ縮約する。
fn turning_points(series: &[f64]) -> Vec<f64> {
    let mut pts: Vec<f64> = Vec::with_capacity(series.len());
    for &v in series {
        if pts.last().map(|&l| l != v).unwrap_or(true) {
            pts.push(v);
        }
    }
    if pts.len() < 2 {
        return pts;
    }
    let mut tp = Vec::with_capacity(pts.len());
    tp.push(pts[0]);
    for i in 1..pts.len() - 1 {
        let a = pts[i] - pts[i - 1];
        let b = pts[i + 1] - pts[i];
        if a * b < 0.0 {
            tp.push(pts[i]);
        }
    }
    tp.push(pts[pts.len() - 1]);
    tp
}

/// レインフロー計数（ASTM E1049-85 3 点法）。振れ幅（range）のサイクル列を返す。
pub fn rainflow_cycles(series: &[f64]) -> Vec<RainflowCycle> {
    let tp = turning_points(series);
    let mut stack: Vec<f64> = Vec::new();
    let mut cycles: Vec<RainflowCycle> = Vec::new();
    for x in tp {
        stack.push(x);
        while stack.len() >= 3 {
            let n = stack.len();
            let y = (stack[n - 2] - stack[n - 3]).abs();
            let xr = (stack[n - 1] - stack[n - 2]).abs();
            if xr < y {
                break;
            }
            if n == 3 {
                cycles.push(RainflowCycle {
                    range: y,
                    count: 0.5,
                });
                stack.remove(0);
            } else {
                cycles.push(RainflowCycle {
                    range: y,
                    count: 1.0,
                });
                stack.remove(n - 2);
                stack.remove(n - 3);
            }
        }
    }
    for i in 0..stack.len().saturating_sub(1) {
        cycles.push(RainflowCycle {
            range: (stack[i + 1] - stack[i]).abs(),
            count: 0.5,
        });
    }
    cycles
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_turning_points_filters_intermediate() {
        let tp = turning_points(&[0.0, 1.0, 2.0, 1.0, 3.0]);
        assert_eq!(tp, vec![0.0, 2.0, 1.0, 3.0]);
    }

    #[test]
    fn test_rainflow_single_half_cycle() {
        let c = rainflow_cycles(&[0.0, 2.0, 0.0]);
        assert_eq!(c.len(), 2);
        let total: f64 = c.iter().map(|x| x.count).sum();
        assert!((total - 1.0).abs() < 1e-9);
        assert!(c.iter().all(|x| (x.range - 2.0).abs() < 1e-9));
    }

    #[test]
    fn test_rainflow_nested_cycle() {
        // 内側の小サイクル(範囲2)が全サイクル、外側(範囲4)が半サイクル2回。
        let c = rainflow_cycles(&[0.0, 3.0, 1.0, 4.0, 0.0]);
        let full2 = c
            .iter()
            .filter(|x| (x.range - 2.0).abs() < 1e-9 && (x.count - 1.0).abs() < 1e-9)
            .count();
        assert_eq!(full2, 1, "nested range-2 full cycle expected: {c:?}");
        let half4: f64 = c
            .iter()
            .filter(|x| (x.range - 4.0).abs() < 1e-9)
            .map(|x| x.count)
            .sum();
        assert!((half4 - 1.0).abs() < 1e-9, "range-4 total count: {half4}");
    }
}
