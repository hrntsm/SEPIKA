//! RC 部材の構造規定（マニュアル 2.5.2 / 2.5.3）と梁の長期たわみ略算。
//!
//! - **入力エラー** → [`CheckKind::Provision`]（検定比 > 1 で NG）
//! - **警告** → 共通 detail へ付記（検定比には載せない）
//!
//! 計算ルート依存の項目（割増率下限・pw のルート別下限）はルート設定が未実装のため
//! 対象外（pw はルート1/3 相当の 0.2% 下限のみ警告）。

use sepika_core::model::Material;
use sepika_core::section_shape::SectionShape;
use sepika_core::units::ConcreteClass;

use super::section_props::{axis_props_from_shape, rebar_info_from_shape, AxisProps, RcRebarInfo};
use crate::ultimate::rc_props::RcDirection;
use crate::{CheckComponent, CheckKind, DesignCtx};

/// 構造規定の判定結果。
pub(crate) struct ProvisionCheck {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl ProvisionCheck {
    pub(crate) fn provision_component(&self) -> Option<CheckComponent> {
        if self.errors.is_empty() {
            return None;
        }
        Some(CheckComponent {
            kind: CheckKind::Provision,
            ratio: 1.0 + self.errors.len() as f64 * 0.01,
            detail: format!("構造規定エラー: {}", self.errors.join(" / ")),
        })
    }

    pub(crate) fn warning_suffix(&self) -> String {
        if self.warnings.is_empty() {
            String::new()
        } else {
            format!("; ⚠ {}", self.warnings.join(" / "))
        }
    }
}

/// 部材レベルの項目を付記する位置か。
///
/// 部材中央（pos≈0.5）に付記する。
pub(crate) fn is_member_level_station(pos: f64) -> bool {
    (pos - 0.5).abs() < 1e-6
}

pub(crate) fn column_member_provisions(
    shape: &SectionShape,
    mat: &Material,
    ctx: &DesignCtx,
) -> Option<ProvisionCheck> {
    let (d_min, gross_area) = match *shape {
        SectionShape::RcColumnRect { b, d, .. } => (b.min(d), b * d),
        SectionShape::RcColumnCircle { d, .. } => (d, std::f64::consts::PI * d * d / 4.0),
        _ => return None,
    };
    let pw = axis_props_from_shape(shape, RcDirection::Strong, true)?
        .pw
        .min(axis_props_from_shape(shape, RcDirection::Weak, true)?.pw);
    let info = rebar_info_from_shape(shape, true)?;
    Some(column_provisions_info(
        &info,
        d_min,
        ctx.clear_length.filter(|&l| l > 1e-9).unwrap_or(ctx.length),
        mat.concrete_class,
        gross_area,
        info.main_area,
        pw,
    ))
}

/// RC 梁の構造規定（マニュアル 2.5.2(5)）。
pub(crate) fn beam_provisions(
    d_full: f64,
    props: &AxisProps,
    info: &RcRebarInfo,
    long_term: bool,
    mz_abs: f64,
    ft: Option<f64>,
    check_stirrup_spacing: bool,
) -> ProvisionCheck {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    if info.cover + 1e-9 < 30.0 {
        errors.push(format!(
            "かぶり厚 {:.0} mm < 30 mm（入力エラー）",
            info.cover
        ));
    }
    let pitch = info.shear_pitch;
    if check_stirrup_spacing && pitch > 0.0 {
        let lim_err = (0.75 * d_full).min(300.0);
        if pitch > lim_err + 1e-9 {
            errors.push(format!(
                "あばら間隔 {:.0} mm > min(3/4·D, 300)={:.0} mm（入力エラー）",
                pitch, lim_err
            ));
        }
        let lim_warn = (0.5 * d_full).min(250.0);
        if pitch > lim_warn + 1e-9 {
            warnings.push(format!(
                "あばら間隔 {:.0} mm > min(D/2, 250)={:.0} mm",
                pitch, lim_warn
            ));
        }
    }
    if info.main_dia.designation_or_diameter_mm() + 1e-9 < 13.0 {
        warnings.push(format!("主筋径 {} < D13", info.main_dia));
    }
    if check_stirrup_spacing && props.pw + 1e-12 < 0.002 {
        warnings.push(format!("pw={:.4} < 0.2%", props.pw));
    }
    if let Some(ft) =
        ft.filter(|&ft| long_term && ft > 0.0 && props.j > 0.0 && props.d > 0.0 && props.b > 0.0)
    {
        let at_min_geom = 0.004 * props.b * props.d;
        let at_req = if mz_abs > 0.0 {
            mz_abs / (ft * props.j)
        } else {
            0.0
        };
        let at_need = if at_req > 0.0 {
            at_min_geom.min((4.0 / 3.0) * at_req)
        } else {
            at_min_geom
        };
        if props.at + 1e-9 < at_need {
            warnings.push(format!(
                "引張鉄筋 at={:.0} mm² < min(0.004bd, 4/3·必要量)={:.0} mm²",
                props.at, at_need
            ));
        }
    }

    ProvisionCheck { errors, warnings }
}

/// 実配筋モデル柱の構造規定。
///
/// `length` は支点間距離 [mm]。呼び出し側は内法（`DesignCtx.clear_length`）を
/// 優先して渡す。`pw_min` は検討方向のせん断補強筋比の最小値。
pub(crate) fn column_provisions_info(
    info: &RcRebarInfo,
    d_min: f64,
    length: f64,
    concrete_class: ConcreteClass,
    ag: f64,
    as_total: f64,
    pw_min: f64,
) -> ProvisionCheck {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    if info.cover + 1e-9 < 30.0 {
        errors.push(format!(
            "かぶり厚 {:.0} mm < 30 mm（入力エラー）",
            info.cover
        ));
    }

    let min_bars = if info.is_circle { 8u32 } else { 4u32 };
    if info.main_count < min_bars {
        errors.push(format!(
            "主筋全本数 {} < 最小 {min_bars}（入力エラー）",
            info.main_count
        ));
    }

    if info.shear_pitch > 100.0 + 1e-9 {
        errors.push(format!(
            "帯筋間隔 {:.0} mm > 100 mm（入力エラー）",
            info.shear_pitch
        ));
    }

    if d_min > 0.0 && length > 0.0 {
        let ratio = d_min / length;
        let lim = match concrete_class {
            ConcreteClass::Normal | ConcreteClass::UserDefined => 1.0 / 15.0,
            ConcreteClass::Lightweight1 | ConcreteClass::Lightweight2 => 1.0 / 10.0,
        };
        if ratio + 1e-12 < lim {
            warnings.push(format!("最小径/支点間距離={:.4} < {:.4}", ratio, lim));
        }
    }

    if ag > 0.0 {
        let pg = as_total / ag;
        if pg + 1e-12 < 0.008 {
            warnings.push(format!("主筋比 pg={:.3}% < 0.8%", pg * 100.0));
        }
    }

    if pw_min + 1e-12 < 0.002 {
        warnings.push(format!(
            "pw={:.4} < 0.2%（ルート未連動・下限 0.2%）",
            pw_min
        ));
    }

    ProvisionCheck { errors, warnings }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sepika_core::section_shape::one_bar_area;

    fn sample_info(cover: f64, pitch: f64, dia: f64, count: u32) -> RcRebarInfo {
        RcRebarInfo {
            cover,
            main_dia: sepika_core::section_shape::RebarSize::DiameterMm(dia),
            main_count: count,
            main_area: count as f64 * one_bar_area(dia),
            tension_count: count,
            tension_layers: 1,
            tension_first_layer_count: count as f64,
            tension_count_1991: (count as f64 / 2.0).max(1.0),
            shear_dia: sepika_core::section_shape::RebarSize::DiameterMm(10.0),
            shear_pitch: pitch,
            shear_legs: 2,
            is_circle: false,
        }
    }

    #[test]
    fn beam_cover_error_and_pitch_error() {
        let info = sample_info(20.0, 400.0, 19.0, 4);
        let props = AxisProps {
            b: 300.0,
            d_full: 600.0,
            dt: 50.0,
            d: 550.0,
            at: 1000.0,
            ac: 1000.0,
            j: 7.0 * 550.0 / 8.0,
            pw: 0.003,
        };
        let p = beam_provisions(600.0, &props, &info, true, 0.0, Some(195.0), true);
        assert!(p.errors.iter().any(|e| e.contains("かぶり")));
        assert!(p.errors.iter().any(|e| e.contains("あばら間隔")));
    }

    #[test]
    fn column_pw_uses_min_of_both_widths() {
        let info = sample_info(40.0, 250.0, 22.0, 4);
        let p = column_provisions_info(
            &info,
            400.0,
            4000.0,
            ConcreteClass::Normal,
            800.0 * 400.0,
            info.main_area,
            2.0 * one_bar_area(10.0) / (800.0 * 250.0),
        );
        // pw_d = 2·(π·5²)/(400·250) ≈ 0.00157 < 0.002
        assert!(
            p.warnings
                .iter()
                .any(|w| w.contains("pw=") && w.contains("0.2%")),
            "warnings={:?}",
            p.warnings
        );
    }

    #[test]
    fn column_slenderness_uses_passed_span_length() {
        // d_min/L: 400/7000 < 1/15 → 警告、400/5000 > 1/15 → なし
        let info = sample_info(40.0, 100.0, 22.0, 8);
        let ag = 400.0 * 400.0;
        let slender = column_provisions_info(
            &info,
            400.0,
            7000.0,
            ConcreteClass::Normal,
            ag,
            info.main_area,
            2.0 * one_bar_area(10.0) / (400.0 * 100.0),
        );
        assert!(
            slender
                .warnings
                .iter()
                .any(|w| w.contains("最小径/支点間距離")),
            "warnings={:?}",
            slender.warnings
        );
        let ok = column_provisions_info(
            &info,
            400.0,
            5000.0,
            ConcreteClass::Normal,
            ag,
            info.main_area,
            2.0 * one_bar_area(10.0) / (400.0 * 100.0),
        );
        assert!(
            ok.warnings.iter().all(|w| !w.contains("最小径/支点間距離")),
            "warnings={:?}",
            ok.warnings
        );
    }
}
