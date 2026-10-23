//! 偏心率の厳密計算コア（武藤 D 値法の閉形式）。
//!
//! - [`ColumnStiffness`] — 柱1本の平面位置と方向別水平剛性（D値）。
//! - [`d_value`] — 武藤 D 値の閉形式。
//! - [`center_of_rigidity`] — 剛心座標 [Xs, Ys]。
//! - [`Eccentricity`] — 偏心率の算定結果。
//! - [`eccentricity`] — 剛心・重心・柱剛性から偏心率を算定。

/// 1 本の柱（鉛直部材）の、平面位置と方向別水平剛性（D値）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColumnStiffness {
    /// 柱の平面位置 (x, y) `[mm]`。
    pub pos: [f64; 2],
    /// X 加力方向の水平剛性 Dx [N/mm]。
    pub dx: f64,
    /// Y 加力方向の水平剛性 Dy [N/mm]。
    pub dy: f64,
}

/// 武藤 D 値の閉形式。加力方向ごとに呼ぶ。
///
/// - `e`: ヤング係数 [N/mm²]
/// - `ic`: 加力方向の柱断面二次モーメント `[mm⁴]`
/// - `h`: 階高（柱長）`[mm]`
/// - `sum_beam_stiffness_ratio`: 柱頭・柱脚に取り付く、加力方向に効く梁の剛比 ΣKb（= Σ Ib/Lb）
/// - `first_story`: 最下階（柱脚固定）なら true。一般階は false。
///
/// ```text
/// Kc0 = 12·E·Ic/h³,  kc = Ic/h,  k̄ = ΣKb/(2·kc)
/// a   = k̄/(2+k̄)            （一般階）
///     = (0.5+k̄)/(2+k̄)      （最下階・柱脚固定）
/// D   = a · Kc0
/// ```
pub fn d_value(e: f64, ic: f64, h: f64, sum_beam_stiffness_ratio: f64, first_story: bool) -> f64 {
    if h <= 0.0 || ic <= 0.0 {
        return 0.0;
    }
    let kc0 = 12.0 * e * ic / (h * h * h);
    let kc = ic / h;
    if kc <= 0.0 {
        return 0.0;
    }
    let kbar = sum_beam_stiffness_ratio / (2.0 * kc);
    let a = if first_story {
        (0.5 + kbar) / (2.0 + kbar)
    } else {
        kbar / (2.0 + kbar)
    };
    a * kc0
}

/// 剛心座標 `[mm]` を返す。方向の剛性合計が 0 なら、その座標は 0。
/// 剛性・位置一次モーメントの集計又は除算が非有限なら Err。
pub fn center_of_rigidity(cols: &[ColumnStiffness]) -> Result<[f64; 2], String> {
    let sum_dy: f64 = cols.iter().map(|c| c.dy).sum();
    let sum_dx: f64 = cols.iter().map(|c| c.dx).sum();
    let sum_dy_x: f64 = cols.iter().map(|c| c.dy * c.pos[0]).sum();
    let sum_dx_y: f64 = cols.iter().map(|c| c.dx * c.pos[1]).sum();
    if [sum_dx, sum_dy, sum_dy_x, sum_dx_y]
        .iter()
        .any(|value| !value.is_finite())
    {
        return Err("Re: 方向別剛性又は剛心の位置一次モーメントを集計できません".into());
    }
    let xs = if sum_dy == 0.0 {
        0.0
    } else {
        sum_dy_x / sum_dy
    };
    let ys = if sum_dx == 0.0 {
        0.0
    } else {
        sum_dx_y / sum_dx
    };
    if !xs.is_finite() || !ys.is_finite() {
        return Err("Re: 剛心座標を算定できません".into());
    }
    Ok([xs, ys])
}

/// 偏心率の算定結果（X 加力・Y 加力）。
#[derive(Clone, Debug, PartialEq)]
pub struct Eccentricity {
    /// 偏心距離 ex = |Xg − Xs| `[mm]`。
    pub ex: f64,
    /// 偏心距離 ey = |Yg − Ys| `[mm]`。
    pub ey: f64,
    /// ねじり剛性 KR = Σ(Dx·ȳ²) + Σ(Dy·x̄²)（剛心まわり）。
    pub kr: f64,
    /// 弾力半径 rex = √(KR/ΣDx)。
    pub rex: f64,
    /// 弾力半径 rey = √(KR/ΣDy)。
    pub rey: f64,
    /// X 加力時の偏心率 Rex。半径・距離が不正又は演算不能なら Err。
    pub re_x: Result<f64, String>,
    /// Y 加力時の偏心率 Rey。半径・距離が不正又は演算不能なら Err。
    pub re_y: Result<f64, String>,
}

/// 剛心・重心 `[mm]` と柱剛性から偏心率を返す。
/// 偏心距離・方向別剛性・ねじり剛性・弾力半径の演算が非有限なら Err。
pub fn eccentricity(
    cols: &[ColumnStiffness],
    center_of_mass: [f64; 2],
    center_of_rigidity: [f64; 2],
) -> Result<Eccentricity, String> {
    let [xs, ys] = center_of_rigidity;
    let [xg, yg] = center_of_mass;
    let ex = (xg - xs).abs();
    let ey = (yg - ys).abs();

    let sum_dx: f64 = cols.iter().map(|c| c.dx).sum();
    let sum_dy: f64 = cols.iter().map(|c| c.dy).sum();

    let kr: f64 = cols
        .iter()
        .map(|c| {
            let xbar = c.pos[0] - xs;
            let ybar = c.pos[1] - ys;
            c.dx * ybar * ybar + c.dy * xbar * xbar
        })
        .sum();

    if [ex, ey, sum_dx, sum_dy, kr]
        .iter()
        .any(|value| !value.is_finite())
    {
        return Err("Re: 偏心距離・方向別剛性又はねじり剛性を算定できません".into());
    }

    let rex = if sum_dx > 0.0 {
        (kr / sum_dx).sqrt()
    } else {
        0.0
    };
    let rey = if sum_dy > 0.0 {
        (kr / sum_dy).sqrt()
    } else {
        0.0
    };
    if !rex.is_finite() || !rey.is_finite() {
        return Err("Re: 弾力半径を算定できません".into());
    }
    let re_x = super::super::holding_capacity::eccentricity_ratio(ey, rex);
    let re_y = super::super::holding_capacity::eccentricity_ratio(ex, rey);

    Ok(Eccentricity {
        ex,
        ey,
        kr,
        rex,
        rey,
        re_x,
        re_y,
    })
}
