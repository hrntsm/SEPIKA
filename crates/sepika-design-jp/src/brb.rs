//! 座屈補剛ブレース（BRB: Buckling Restrained Brace）の断面検定
//! （JFEシビル二重鋼管座屈補剛ブレース／日鉄アンボンドブレース等、メーカーの
//! 性能評価・大臣認定による製品固有許容値に基づく断面検定）。
//!
//! BRB は芯材が座屈拘束されているため一般の鋼構造ブレースのような座屈を
//! 考慮した許容圧縮応力度 fc（[`crate::steel::steel_fc`]）は用いず、メーカーが
//! 性能評価・大臣認定等で定めた製品固有の許容値を直接用いる。検定項目は
//! 次の 2 つ:
//!
//! 1. 短期許容軸力 ≧ 設計軸力（軸力検定）
//! 2. 座屈長さ ≦ 限界座屈長さ（座屈長さ検定）
//!
//! 座屈長さは芯材の全長からブレース端部の座屈長さ低減距離を差し引いた
//! `Lk = L − 2・L1` で求める（`L1` はブレース上下端それぞれの低減距離
//! `L1上`, `L1下` の平均値 `(L1上+L1下)/2` として入力する）。

use crate::{CheckComponent, CheckKind, CheckResult};
use sepika_core::model::BrbAttr;

/// BRB の断面検定（軸力・座屈長さの 2 項目）。
///
/// `n_design` は部材軸力 `[N]`（絶対値で評価）、`member_length` は実材軸長 `[mm]`。
/// 製品値の不足・不正、または実材軸長・控除後の長さの不正は理由付きエラー。
pub fn brb_check(
    attr: &BrbAttr,
    n_design: f64,
    member_length: f64,
    long_term: bool,
) -> Result<CheckResult, String> {
    let allowable_axial_short_n =
        product_value(attr.allowable_axial_short, "短期許容軸力 Na", false)?;
    let critical_length_mm = product_value(attr.critical_length, "限界座屈長さ Lc", false)?;
    let length_reduction_mm = product_value(attr.length_reduction, "端部低減距離 L1", true)?;
    validate_member_length(member_length)?;
    let end_reduction_mm = 2.0 * length_reduction_mm;
    let buckling_length_mm = member_length - end_reduction_mm;
    if buckling_length_mm <= 0.0 {
        return Err("BRBの控除後の座屈長さ L−2L1 は正値が必要です".into());
    }

    let na = if long_term {
        allowable_axial_short_n / 1.5
    } else {
        allowable_axial_short_n
    };
    let demand_n = n_design.abs();
    let ratio_axial = if long_term && na < f64::MIN_POSITIVE {
        // 非正規化数まで小さくなった長期許容軸力を分母にすると、丸めで軸力比が過小になり得る。
        (demand_n / allowable_axial_short_n) * 1.5
    } else {
        demand_n / na
    };
    let ratio_axial = if long_term
        && ratio_axial == 1.0
        && exceeds_long_term_limit(demand_n, allowable_axial_short_n)
    {
        1.0_f64.next_up()
    } else {
        ratio_axial
    };
    let ratio_length = buckling_length_mm / critical_length_mm;
    let ratio_length = if ratio_length == 1.0
        && exceeds_length_limit(member_length, end_reduction_mm, critical_length_mm)
    {
        1.0_f64.next_up()
    } else {
        ratio_length
    };

    let ratio = ratio_axial.max(ratio_length);
    // 比を有限値で保持しないと、結果表示が上限超過を未検定として扱う。
    let ratio_overflow = n_design.is_finite() && ratio.is_infinite();
    let term_label = if long_term { "長期" } else { "短期" };
    let mut detail = format!(
        "{}: |N|={:.4} N, Na={:.4} N, N/Na={:.4} / Lk={:.4} mm, 限界座屈長={:.4} mm, Lk/限界座屈長={:.4}",
        term_label, demand_n, na, ratio_axial, buckling_length_mm, critical_length_mm, ratio_length
    );
    if ratio_overflow {
        detail.push_str(
            "。検定比の演算が数値表現の上限に達したため、総合検定比に表現可能な最大値を表示します",
        );
    }

    Ok(CheckResult {
        basis: "座屈補剛ブレース（メーカー許容値による検定）".to_string(),
        detail: String::new(),
        components: vec![CheckComponent {
            kind: CheckKind::Axial,
            ratio: if ratio_overflow { f64::MAX } else { ratio },
            detail,
        }],
    })
}

fn exceeds_long_term_limit(demand_n: f64, short_limit_n: f64) -> bool {
    // 差を一度に求め、軸力比と長期許容軸力の丸めで超過を見落とさない。
    // 極小域では差が最小の非正規化数より小さくなり得るので、先に正確に拡大する。
    let scale = if short_limit_n < 4.0 * f64::MIN_POSITIVE {
        (1_u64 << 53) as f64
    } else {
        1.0
    };
    (demand_n * scale).mul_add(1.5, -(short_limit_n * scale)) > 0.0
}

fn exceeds_length_limit(length_mm: f64, end_reduction_mm: f64, limit_mm: f64) -> bool {
    let larger_mm = end_reduction_mm.max(limit_mm);
    let smaller_mm = end_reduction_mm.min(limit_mm);
    // 大きい項がLの半分以上なら、境界付近の差は丸めずに求められる。
    2.0 * larger_mm < length_mm || length_mm - larger_mm > smaller_mm
}

fn product_value(value: Option<f64>, name: &str, allow_zero: bool) -> Result<f64, String> {
    let value = value.ok_or_else(|| format!("BRBの{name}が未入力です"))?;
    if !value.is_finite() || value < 0.0 || (!allow_zero && value == 0.0) {
        let range = if allow_zero { "非負値" } else { "正値" };
        return Err(format!("BRBの{name}には有限の{range}が必要です"));
    }
    Ok(value)
}

pub(crate) fn validate_member_length(length_mm: f64) -> Result<(), String> {
    if !length_mm.is_finite() || length_mm <= 0.0 {
        return Err("BRBの実材軸長 L には有限の正値が必要です".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sepika_core::ids::ElemId;

    fn attr() -> BrbAttr {
        BrbAttr {
            elem: ElemId(1),
            allowable_axial_short: Some(1_000_000.0),
            critical_length: Some(4000.0),
            length_reduction: Some(300.0),
        }
    }

    /// 手計算: N=500,000N（短期）、Na=1,000,000N → N/Na=0.5。
    /// L=5000, L1=300 → Lk=5000-600=4400, 限界=4000 → Lk/限界=1.1。
    /// ratio = max(0.5, 1.1) = 1.1 → NG。
    #[test]
    fn test_brb_check_hand_calc_length_governs() {
        let a = attr();
        let result = brb_check(&a, 500_000.0, 5000.0, false).unwrap();
        assert!(
            (result.ratio() - 1.1).abs() < 1e-6,
            "ratio={}",
            result.ratio()
        );
        assert!(!result.ok());
    }

    /// 軸力が支配するケース: N=1,200,000N（短期許容 1,000,000N 超過）、
    /// L=3000（Lk=2400 < 4000 なので座屈長さは余裕あり）。
    /// N/Na=1.2、Lk/限界=2400/4000=0.6 → ratio=1.2 → NG。
    #[test]
    fn test_brb_check_axial_governs_and_fails() {
        let a = attr();
        let result = brb_check(&a, 1_200_000.0, 3000.0, false).unwrap();
        assert!(
            (result.ratio() - 1.2).abs() < 1e-6,
            "ratio={}",
            result.ratio()
        );
        assert!(!result.ok());
    }

    /// 長期は短期許容値の 1/1.5 を用いる: 同じ N でも長期の方が検定比が
    /// 1.5 倍厳しくなる（座屈長さ検定比は変わらないため軸力支配ケースで検証）。
    #[test]
    fn test_brb_check_long_term_uses_1_5_divisor() {
        let a = attr();
        let n = 300_000.0;
        // Lk=1000-600=400、座屈長さ検定比=400/4000=0.1 は軸力検定比より
        // 十分小さく保ち、軸力検定が支配するようにする。
        let l = 1000.0;
        let ratio_short = brb_check(&a, n, l, false).unwrap().ratio();
        let ratio_long = brb_check(&a, n, l, true).unwrap().ratio();
        assert!(
            (ratio_short - 0.3).abs() < 1e-9,
            "ratio_short={}",
            ratio_short
        );
        assert!(
            (ratio_long - 0.45).abs() < 1e-9,
            "ratio_long={}",
            ratio_long
        );
        assert!(brb_check(&a, n, l, true).unwrap().ok());
    }
}
