//! RC 部材の終局強度（曲げ・せん断）ヘルパ群。
//!
//! - [`biaxial_margin`] — 2 軸相互作用の余裕度。
//! - [`column_mu`] — 柱の曲げ終局強度 Mu（構造規定 at 式）。
//! - [`member_shear_strength`] — 選択式に応じた終局せん断強度 Qsu/Vu。

use super::options::{ShearMethod, UltimateShearOptions};
use super::rc_props::RcBarProps;
use super::rc_shear::{rc_shear_qsu_plastic_with_strength, RcPlasticShearInput};
use super::rc_shear_ductility::{rc_shear_vu_ductility_with_strength, RcDuctilityShearInput};
use sepika_core::rc_capacity::{rc_column_mu_simple, RcCapacityInput};

/// 2 軸相互作用の余裕度 `1/((rx)^α + (ry)^α)^(1/α)`（採用応力）。
///
/// `rx`,`ry` は各軸の「需要/耐力」比（例: `Qmx/Qux`, `Qmy/Quy`）、`alpha` は相互作用の
/// 指数（RC 柱は 2.0）。ここでは αx=αy=α と等しく扱う。両比が 0 のとき（需要ゼロ）は
/// `f64::INFINITY` を返す。`alpha ≤ 0` の不正入力も `f64::INFINITY`。
pub fn biaxial_margin(rx: f64, ry: f64, alpha: f64) -> f64 {
    if alpha <= 0.0 {
        return f64::INFINITY;
    }
    let rx = rx.max(0.0);
    let ry = ry.max(0.0);
    let s = rx.powf(alpha) + ry.powf(alpha);
    if s <= 0.0 {
        f64::INFINITY
    } else {
        1.0 / s.powf(1.0 / alpha)
    }
}

/// 柱の曲げ終局強度 Mu [N·mm]（構造規定 at 式、軸力考慮）。
/// `b_dir`=幅, `d_dir`=せい, `dt`=引張縁〜引張筋距離, `at`=引張側主筋, `ag`=全主筋。
#[allow(clippy::too_many_arguments)]
pub(super) fn column_mu(
    b_dir: f64,
    d_dir: f64,
    dt: f64,
    at: f64,
    ag: f64,
    sigma_y: f64,
    fc: f64,
    n_axial: f64,
) -> f64 {
    let cap = RcCapacityInput {
        b: b_dir,
        d: d_dir,
        at,
        d_eff: (d_dir - dt).max(1.0),
        sigma_y,
        fc,
        pw: 0.0,
        sigma_wy: 0.0,
        clear_span: 1.0,
        sigma_0: 0.0,
    };
    rc_column_mu_simple(&cap, ag, n_axial)
}

/// 選択された [`ShearMethod`] に応じた終局せん断強度 `Qsu`/`Vu` [N]。
pub(super) fn member_shear_strength(
    props: &RcBarProps,
    fc: f64,
    n_axial: f64,
    l_clear: f64,
    sigma_wy: Option<f64>,
    opts: &UltimateShearOptions,
) -> Option<f64> {
    let jt = 7.0 * props.d_eff / 8.0;
    match opts.shear_method {
        ShearMethod::Plastic => rc_shear_qsu_plastic_with_strength(
            &RcPlasticShearInput {
                b: props.b_dir,
                d_full: props.d_dir,
                jt,
                pw: props.pw,
                sigma_wy: sigma_wy.unwrap_or(f64::NAN),
                l_clear,
                fc,
                rp: opts.rp,
                lightweight: opts.lightweight,
            },
            sigma_wy,
        ),
        ShearMethod::Ductility => {
            let s = props.shear_pitch;
            let aw = props.shear_legs as f64 * props.shear_dia.area_mm2();
            let pwe = if s > 0.0 { aw / (props.be * s) } else { 0.0 };
            rc_shear_vu_ductility_with_strength(
                &RcDuctilityShearInput {
                    b: props.b_dir,
                    d_full: props.d_dir,
                    be: props.be,
                    je: jt,
                    pwe,
                    sigma_wy: sigma_wy.unwrap_or(f64::NAN),
                    s,
                    n_s: props.n_s,
                    l_clear,
                    fc,
                    rp: opts.rp,
                    tensile_axial: n_axial < 0.0,
                    lightweight: opts.lightweight,
                },
                sigma_wy,
            )
        }
    }
}
