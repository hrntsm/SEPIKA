//! SRC 造の部材種別（保有水平耐力計算の Ds 算定、技術基準解説書 表 2.6.6-5）。
//!
//! - [`src_column_rank`] — SRC 柱の部材種別（曲げ・せん断破壊モード別）
//! - [`src_wall_type`] — SRC 耐震壁の種別（せん断破壊 WC・それ以外 WA）
//! - [`src_column_rank_ratios`] — 判定比 N/N0・sM0/M0 の算定

use super::holding_capacity::MemberRank;
use sepika_core::model::{Material, MaterialCategory, Section};
use sepika_core::rc_rebar_geom::RectEdge;
use sepika_core::section_shape::SectionShape;

/// SRC 柱の部材種別（表 2.6.6-5）。
///
/// - `n_over_n0`: メカニズム時軸力 N（圧縮正）/ SRC 断面の圧縮耐力 N0。
/// - `smo_over_m0`: 鉄骨の曲げ耐力 sM0 / SRC 断面の曲げ耐力 M0（N=0）。
/// - `shear_failure`: 破壊モードがせん断破壊か（増分解析のせん断降伏イベント）。
///
/// せん断破壊は同じ (N/N0, sM0/M0) 区分で曲げ破壊より 1 段階不利になる
/// （FA→FB、FC→FD。N/N0 > 0.4 は破壊モードによらず FD）。
pub fn src_column_rank(n_over_n0: f64, smo_over_m0: f64, shear_failure: bool) -> MemberRank {
    if n_over_n0 > 0.4 {
        return MemberRank::FD;
    }
    let base: u8 = match (n_over_n0 > 0.3, smo_over_m0 < 0.4) {
        (false, false) => 0,
        (false, true) | (true, false) => 1,
        (true, true) => 2,
    };
    let idx = base + u8::from(shear_failure);
    match idx {
        0 => MemberRank::FA,
        1 => MemberRank::FB,
        2 => MemberRank::FC,
        _ => MemberRank::FD,
    }
}

/// SRC 耐震壁の種別。破壊モードがせん断破壊の場合を WC、それ以外を WA とする。
///
/// 返り値は Ds 表の行選択に用いるグループ表現（0=A(WA)…2=C(WC)。
/// [`super::ds_group`] の規約と同じく [`MemberRank`] のインデックスで表す）。
pub fn src_wall_type(shear_failure: bool) -> MemberRank {
    if shear_failure {
        MemberRank::FC
    } else {
        MemberRank::FA
    }
}

/// SRC 矩形柱の部材種別用の候補値 `(N/N0, sM0/M0)` を返す。
/// `fc` と `rebar_sy` は N/mm²、`n_ult` は N（圧縮正、引張は 0 扱い）。
/// 入力不備や割当鉄骨材料と実板厚から基準強度を確定できない場合は理由を返す。
pub fn src_column_rank_ratios(
    section: &Section,
    steel_material: Option<&Material>,
    fc: f64,
    rebar_sy: f64,
    n_ult: f64,
) -> Result<(f64, f64), String> {
    let Some(SectionShape::SrcColumnRect {
        b,
        d,
        ref rebar,
        steel_height: sh,
        steel_width: sb,
        steel_web_thick: tw,
        steel_flange_thick: tf,
    }) = section.shape
    else {
        return Err("SRC 矩形柱の断面形状が必要です".into());
    };
    let steel = steel_material.ok_or("内蔵鉄骨の材料が未割当、または参照先が不明です")?;
    if steel.category != MaterialCategory::Steel {
        return Err(format!("材料「{}」は鋼材ではありません", steel.name));
    }
    let name = steel.name.trim();
    let minimum_mm = match name {
        "SN400A" | "SN400B" | "SN490B" => 6.0,
        "SN400C" | "SN490C" => 16.0,
        "SS400" | "SS490" | "SM400A" | "SM400B" | "SM400C" | "SM490A" | "SM490B" | "SM490C"
        | "SM490YA" | "SM490YB" | "SM520B" | "SM520C" => 0.0,
        "SM400" | "SM490" | "SM520" | "SN400" | "SN490" => {
            return Err(format!("内蔵鉄骨「{name}」の JIS の種類記号が不完全です"));
        }
        _ => {
            return Err(format!(
                "内蔵鉄骨「{name}」の H 形鋼としての適用根拠が未確定です"
            ));
        }
    };
    let s_f = crate::material_strength::section_steel_f(section, Some(steel))?;
    // 断面形状だけでは圧延形鋼と鋼板組立を区別できない。
    if tw < minimum_mm || tf < minimum_mm {
        return Err(format!(
            "内蔵鉄骨「{name}」の実板厚（ウェブ {tw} mm、フランジ {tf} mm）は JIS G 3136 の適用範囲外です。両方に {minimum_mm} mm 以上が必要です"
        ));
    }
    if [fc, rebar_sy, b, d]
        .iter()
        .any(|v| !v.is_finite() || *v <= 0.0)
        || !n_ult.is_finite()
    {
        return Err("SRC 柱の外形寸法・コンクリート強度・主筋強度・軸力が不正です".into());
    }
    if !sh.is_finite() || !sb.is_finite() || sh <= 2.0 * tf || sb <= 0.0 {
        return Err("内蔵 H 形鋼の高さまたは幅が不正です".into());
    }
    rebar
        .validate(b, d)
        .map_err(|reason| format!("SRC 柱の配筋形状が不正です: {reason}"))?;
    let edge = rebar.edge_steel(RectEdge::Top, b, d);

    let hw = sh - 2.0 * tf;
    let s_a = 2.0 * sb * tf + hw * tw;
    let s_zp = sb * tf * (sh - tf) + tw * hw * hw / 4.0;
    let s_m0 = s_zp * s_f;

    let rebar_area = rebar.total_main_area();
    let ac = (b * d - s_a - rebar_area).max(0.0);
    let n0 = ac * fc + s_a * s_f + rebar_area * rebar_sy;
    if !n0.is_finite() || n0 <= 0.0 {
        return Err("SRC 柱の圧縮耐力の候補値を算定できません".into());
    }

    let de = (d - edge.centroid_from_edge_mm).max(0.0);
    let r_m0 = 0.9 * edge.area_mm2 * rebar_sy * de;
    let m0 = s_m0 + r_m0;
    if !m0.is_finite() || m0 <= 0.0 {
        return Err("SRC 柱の曲げ耐力の候補値を算定できません".into());
    }

    Ok((n_ult.max(0.0) / n0, s_m0 / m0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 表 2.6.6-5 の全区分（曲げ破壊・せん断破壊 × 5 列）を検証する。
    #[test]
    fn test_src_column_rank_table_2_6_6_5() {
        assert_eq!(src_column_rank(0.2, 0.5, false), MemberRank::FA);
        assert_eq!(src_column_rank(0.2, 0.3, false), MemberRank::FB);
        assert_eq!(src_column_rank(0.35, 0.5, false), MemberRank::FB);
        assert_eq!(src_column_rank(0.35, 0.3, false), MemberRank::FC);
        assert_eq!(src_column_rank(0.5, 0.5, false), MemberRank::FD);
        assert_eq!(src_column_rank(0.2, 0.5, true), MemberRank::FB);
        assert_eq!(src_column_rank(0.2, 0.3, true), MemberRank::FC);
        assert_eq!(src_column_rank(0.35, 0.5, true), MemberRank::FC);
        assert_eq!(src_column_rank(0.35, 0.3, true), MemberRank::FD);
        assert_eq!(src_column_rank(0.5, 0.3, true), MemberRank::FD);
    }

    /// 境界値: N/N0=0.3・0.4、sM0/M0=0.4 は「以下」「以上」側（有利側）に入る。
    #[test]
    fn test_src_column_rank_boundaries() {
        assert_eq!(src_column_rank(0.3, 0.4, false), MemberRank::FA);
        assert_eq!(src_column_rank(0.4, 0.4, false), MemberRank::FB);
        assert_eq!(src_column_rank(0.4, 0.39, false), MemberRank::FC);
    }

    #[test]
    fn test_src_wall_type() {
        assert_eq!(src_wall_type(false), MemberRank::FA, "WA");
        assert_eq!(src_wall_type(true), MemberRank::FC, "WC");
    }
}
