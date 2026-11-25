//! RC 部材の終局強度（曲げ・せん断）ヘルパ群。
//!
//! - [`biaxial_margin`] — 2 軸相互作用の余裕度。
//! - [`column_mu`] — 柱の曲げ終局強度 Mu（構造規定 at 式）。
//! - [`member_shear_strength`] — 終局せん断強度 Qsu。

use super::rc_props::RcBarProps;
use sepika_core::material_grade::{rebar_f_value, shear_rebar_yield_strength};
use sepika_core::model::Material;
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

/// 荒川mean式の終局せん断強度 [N]。内力・配筋不足は理由を返す。
pub(super) fn member_shear_strength(
    props: &RcBarProps,
    fc: f64,
    axial_n: f64,
    moment_nmm: f64,
    shear_n: f64,
    shear_rebar: Option<&Material>,
) -> Result<f64, &'static str> {
    if !props.at.is_finite() || props.at <= 0.0 || !props.d_eff.is_finite() || props.d_eff <= 0.0 {
        return Err("荒川mean式に必要な引張鉄筋面積または有効せいを確認できません");
    }
    if props.shear_legs == 0 || props.shear_dia.designation_or_diameter_mm() <= 0.0 {
        return Err("検討方向のせん断補強筋を確認できません");
    }
    if !props.shear_pitch.is_finite() || props.shear_pitch <= 0.0 {
        return Err("荒川mean式に必要なせん断補強筋の間隔は正の有限値としてください");
    }
    if !props.pw.is_finite() || props.pw <= 0.0 {
        return Err("せん断補強筋比を確認できません");
    }
    let material = shear_rebar.ok_or("せん断補強筋の降伏強度を確認できません")?;
    let fy = shear_rebar_yield_strength(Some(material))
        .filter(|fy| fy.is_finite())
        .ok_or("せん断補強筋の降伏強度を確認できません")?;
    let grade = material.name.trim();
    let nominal_fy = rebar_f_value(grade).ok_or("せん断補強筋の基準強度を確認できません")?;
    let limit = if grade.starts_with("SR") {
        295.0
    } else {
        490.0
    };
    if fy > limit {
        return Err("荒川mean式のせん断補強筋の強度上限（丸鋼295・異形490 N/mm²）を超えています");
    }
    if fy > nominal_fy {
        return Err("荒川mean式に用いるせん断補強筋の降伏強度が材種の基準強度を超えています");
    }
    if fy < nominal_fy {
        return Err("荒川mean式に用いるせん断補強筋の降伏強度が材種の基準強度を下回っています");
    }
    let shear_span_ratio = if shear_n == 0.0 {
        if moment_nmm == 0.0 {
            return Err("確定総応答のMとQがともにゼロのためせん断スパン比を定められません");
        }
        3.0
    } else {
        (moment_nmm.abs() / shear_n.abs() / props.d_eff).clamp(1.0, 3.0)
    };
    let input = RcCapacityInput {
        b: props.b_dir,
        d: props.d_dir,
        d_eff: props.d_eff,
        at: props.at,
        sigma_y: 0.0,
        fc,
        pw: props.pw,
        sigma_wy: nominal_fy,
        clear_span: 0.0,
        sigma_0: axial_n / (props.b_dir * props.d_dir),
    };
    let qsu = sepika_core::rc_capacity::rc_qsu_arakawa_mean(&input, shear_span_ratio);
    if !qsu.is_finite() || qsu <= 0.0 {
        return Err("終局せん断耐力を有限の正値として算定できません");
    }
    Ok(qsu)
}
