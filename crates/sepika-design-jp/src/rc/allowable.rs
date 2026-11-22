//! RC 検定に用いる許容応力度のまとめ（部材単位で term 依存の値を 1 回だけ計算する）。

use crate::material_strength::{
    concrete_allowable_compression, concrete_allowable_shear_class, rebar_allowable_shear,
    young_ratio_n,
};
pub(crate) use sepika_core::units::ConcreteClass;

pub(super) const UNSUPPORTED_YOUNG_RATIO: &str =
    "RC規準の設計用ヤング係数比はFcが60 N/mm²以下の範囲です";

/// 検定に用いる許容応力度一式。
pub(crate) struct RcAllow {
    /// コンクリート許容圧縮応力度 fc [N/mm²]（長期/短期は算定済み）。
    pub(crate) fc: f64,
    /// コンクリート許容せん断応力度 fs [N/mm²]。
    pub(crate) fs: f64,
    /// せん断補強筋許容引張応力度 w_ft [N/mm²]。
    pub(crate) w_ft: Option<f64>,
    /// ヤング係数比 n。
    pub(crate) n_ratio: Option<f64>,
}

pub(crate) fn rc_allow(
    fc_raw: f64,
    class: ConcreteClass,
    grade: Option<&str>,
    long_term: bool,
) -> RcAllow {
    RcAllow {
        fc: concrete_allowable_compression(fc_raw, long_term),
        fs: concrete_allowable_shear_class(fc_raw, class, long_term),
        w_ft: grade.map(|grade| rebar_allowable_shear(grade, long_term)),
        n_ratio: (fc_raw > 0.0 && fc_raw <= 60.0).then(|| young_ratio_n(fc_raw)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowable_design_young_ratio_stops_at_sixty() {
        let ratio = |fc| rc_allow(fc, ConcreteClass::Normal, Some("SD345"), true).n_ratio;
        assert_eq!(ratio(60.0), Some(9.0));
        assert_eq!(ratio(60.0_f64.next_up()), None);
    }
}
