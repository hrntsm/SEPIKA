//! 床の中での小梁・スラブ（床）の設計。
//!
//! 小梁は大梁を分割せず、床の中で**単純梁または片持ち梁**として検定する。
//!
//! 荷重は床板分配が材軸へ出した辺荷重・小梁自身の自重・架け側の二次部材から
//! 渡された集中荷重の重ね合わせで、二次部材の反力の逐次伝達（`sepika_load::cascade`）が
//! 求める。本モジュールは部材力を受け取って検定するだけである。
//!
//! 単位は N-mm（面荷重 N/mm²、線荷重 N/mm）。

/// 小梁（単純梁・片持ち梁）の設計結果。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BeamDesignResult {
    #[serde(default)]
    pub bolts: Option<crate::floor_bolt::BeamBoltResult>,
    #[serde(default)]
    pub post: Option<crate::post::PostDesignResult>,
    /// スパン（支持間距離）`[mm]`。
    pub span: f64,
    /// 代表等分布荷重 w [N/mm]（合計荷重 / スパン。表示用であり検定には使わない）。
    pub w: f64,
    /// 最大曲げモーメント [N·mm]。
    pub m_max: f64,
    /// 最大せん断力 `[N]`。
    pub q_max: f64,
    /// 曲げ応力度 σ = M/Z [N/mm²]。
    pub sigma: f64,
    /// 許容曲げ応力度 [N/mm²]。
    pub sigma_allow: f64,
    /// 曲げ検定比 σ/σ_allow。
    pub bending_ratio: f64,
    /// たわみ `[mm]`（等分布なら 5wL⁴/(384EI)。二次部材は数値積分）。
    pub deflection: f64,
    /// たわみ比 δ/L。
    pub deflection_span_ratio: f64,
    /// たわみ検定比 (δ/L)/(1/limit_denom)。
    pub deflection_ratio: f64,
    /// 評価済み項目の最大検定比。接合条件不足時もこの値だけで適合としない。
    pub ratio: f64,
    /// 必要な検定が成立し、最大検定比が1以下のとき適合。
    pub ok: bool,
    /// 部材力や必要な接合検定が不足している場合は総合未検定。
    #[serde(default)]
    pub unchecked: bool,
}

/// N-mm 単位の部材力を用いて小梁の曲げ・たわみを検定する（符号は絶対値で評価）。
/// スパン[mm]・断面係数[mm³]・許容応力度[N/mm²]・たわみ限界分母は正の有限値が必要。
/// 荷重・部材力・たわみの非有限値、または計算結果を表現できない場合は理由付きエラー。
#[allow(clippy::too_many_arguments)]
pub fn design_beam_from_forces(
    span: f64,
    w: f64,
    m_max: f64,
    q_max: f64,
    deflection: f64,
    section_modulus: f64,
    sigma_allow: f64,
    defl_limit_denom: f64,
) -> Result<BeamDesignResult, String> {
    judge_beam(
        span,
        w,
        m_max.abs(),
        q_max.abs(),
        deflection.abs(),
        section_modulus,
        sigma_allow,
        defl_limit_denom,
    )
}

/// 共通の検定判定（曲げ応力度・たわみ制限）。
#[allow(clippy::too_many_arguments)]
fn judge_beam(
    span: f64,
    w: f64,
    m_max: f64,
    q_max: f64,
    deflection: f64,
    section_modulus: f64,
    sigma_allow: f64,
    defl_limit_denom: f64,
) -> Result<BeamDesignResult, String> {
    for (name, value) in [
        ("スパン [mm]", span),
        ("断面係数 [mm³]", section_modulus),
        ("許容曲げ応力度 [N/mm²]", sigma_allow),
        ("たわみ限界分母", defl_limit_denom),
    ] {
        if !value.is_finite() || value <= 0.0 {
            return Err(format!("{name}は正の有限値が必要です"));
        }
    }
    for (name, value) in [
        ("代表等分布荷重 [N/mm]", w),
        ("最大曲げモーメント [N·mm]", m_max),
        ("最大せん断力 [N]", q_max),
        ("たわみ [mm]", deflection),
    ] {
        if !value.is_finite() {
            return Err(format!("{name}は有限値が必要です"));
        }
    }
    let sigma = m_max / section_modulus;
    let bending_ratio = sigma / sigma_allow;
    let deflection_span_ratio = deflection / span;
    let deflection_ratio = deflection_span_ratio * defl_limit_denom;
    for (name, value, has_demand) in [
        ("曲げ応力度", sigma, m_max > 0.0),
        ("曲げ検定比", bending_ratio, m_max > 0.0),
        ("たわみ比", deflection_span_ratio, deflection > 0.0),
        ("たわみ検定比", deflection_ratio, deflection > 0.0),
    ] {
        if !value.is_finite() || (has_demand && value == 0.0) {
            return Err(format!("{name}を有限精度で表現できません"));
        }
    }
    let ratio = bending_ratio.max(deflection_ratio);
    Ok(BeamDesignResult {
        bolts: None,
        post: None,
        span,
        w,
        m_max,
        q_max,
        sigma,
        sigma_allow,
        bending_ratio,
        deflection,
        deflection_span_ratio,
        deflection_ratio,
        ratio,
        ok: ratio <= 1.0,
        unchecked: false,
    })
}

/// 分配結果が無く断面検定できない小梁の表行（判定は「未」。OK と誤認しない）。
pub fn beam_unchecked(span: f64) -> BeamDesignResult {
    BeamDesignResult {
        bolts: None,
        post: None,
        span,
        w: 0.0,
        m_max: 0.0,
        q_max: 0.0,
        sigma: 0.0,
        sigma_allow: 0.0,
        bending_ratio: 0.0,
        deflection: 0.0,
        deflection_span_ratio: 0.0,
        deflection_ratio: 0.0,
        ratio: 0.0,
        ok: false,
        unchecked: true,
    }
}

/// スラブ（一方向）の設計結果。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SlabDesignResult {
    /// 設計スパン（短辺）`[mm]`。
    pub span: f64,
    /// 面荷重 w [N/mm²]。
    pub w: f64,
    /// 単位幅あたり設計曲げモーメント M = wL²/coef [N·mm/mm]。
    pub moment: f64,
    /// 板厚 t `[mm]`。
    pub thickness: f64,
    /// 有効せい d = t − かぶり `[mm]`。
    pub effective_depth: f64,
    /// 単位幅あたり必要引張鉄筋量 As [mm²/mm]。
    pub as_req_per_mm: f64,
    /// 1m あたり必要引張鉄筋量 As [mm²/m]（表示用）。
    pub as_req_per_m: f64,
}

/// スラブを一方向版として設計し、設計曲げモーメントと必要鉄筋量を算定する。
///
/// - `span`: 設計スパン（短辺）`[mm]`、`w`: 面荷重 [N/mm²]。
/// - `moment_coef`: 曲げモーメント係数（M = wL²/coef。単純支持=8、連続端=10 等）。
/// - `thickness`: 板厚 `[mm]`、`cover`: 圧縮縁から鉄筋重心までのかぶり `[mm]`。
/// - `ft_rebar`: 鉄筋の許容引張応力度 [N/mm²]（長期）、`j_ratio`: 応力中心距離比 j（≒7/8）。
///
/// 必要鉄筋量 As = M / (ft · j · d)。有効せい d が 0 以下なら As=0。
#[allow(clippy::too_many_arguments)]
pub fn design_slab_oneway(
    span: f64,
    w: f64,
    moment_coef: f64,
    thickness: f64,
    cover: f64,
    ft_rebar: f64,
    j_ratio: f64,
) -> SlabDesignResult {
    let coef = if moment_coef > 0.0 { moment_coef } else { 8.0 };
    let moment = w * span * span / coef; // N·mm per mm width
    let d = (thickness - cover).max(0.0);
    let as_req_per_mm = if ft_rebar > 0.0 && j_ratio > 0.0 && d > 0.0 {
        moment / (ft_rebar * j_ratio * d)
    } else {
        0.0
    };
    SlabDesignResult {
        span,
        w,
        moment,
        thickness,
        effective_depth: d,
        as_req_per_mm,
        as_req_per_m: as_req_per_mm * 1000.0,
    }
}

/// たわみ制限の既定分母（δ/L ≤ 1/250）。
pub const DEFLECTION_LIMIT_DENOM: f64 = 250.0;
/// スラブ設計の既定かぶり `[mm]`（圧縮縁〜鉄筋重心）。
pub const SLAB_DEFAULT_COVER: f64 = 30.0;
/// スラブ設計の既定 j 比（応力中心距離 / 有効せい）。
pub const SLAB_J_RATIO: f64 = 7.0 / 8.0;
/// 異形鉄筋 SD295 の長期許容引張応力度 [N/mm²]。
pub const REBAR_FT_LONG_SD295: f64 = 195.0;

/// 小梁略算に使うヤング係数 E [N/mm²] と長期許容曲げ応力度 [N/mm²]。
/// 長期 `ft = F/1.5` を用い、材料・必要実板厚・強度の不備、正の有限値でないEは理由付きエラー。
pub fn beam_steel_e_and_ft(
    sec: &sepika_core::model::Section,
    mat: Option<&sepika_core::model::Material>,
) -> Result<(f64, f64), String> {
    let f = crate::material_strength::section_steel_f(sec, mat)?;
    let m = mat.ok_or("小梁の鋼材が未割当です")?;
    if !m.young.is_finite() || m.young <= 0.0 {
        return Err("ヤング係数Eは正の有限値が必要です".into());
    }
    let e = m.young;
    Ok((e, f / 1.5))
}

/// 鋼小梁の断面、E、長期 ft [N/mm²] を返す。略算対象外は `None`、入力不備は部材ID付きエラー。
pub fn secondary_beam_steel_e_and_ft<'a>(
    model: &'a sepika_core::model::Model,
    member: &sepika_core::model::SecondaryMember,
) -> Result<Option<(&'a sepika_core::model::Section, f64, f64)>, String> {
    use sepika_core::model::SecondaryMemberKind;
    if member.kind != SecondaryMemberKind::Beam || model.secondary_member_materialized(member) {
        return Ok(None);
    }
    let sec = member
        .section
        .and_then(|id| model.section(id))
        .ok_or_else(|| format!("小梁 {}: 断面が未割当または参照先が不明です", member.id.0))?;
    if sec.shape.as_ref().is_some_and(|shape| {
        sepika_core::structure_kind::shape_default_kind(shape)
            != sepika_core::structure_kind::StructureKind::S
    }) {
        return Ok(None);
    }
    let mat = model.secondary_material(member);
    if !crate::member_design_check::is_steel_input_target(Some(sec), mat) {
        return Ok(None);
    }
    let (e, ft) = beam_steel_e_and_ft(sec, mat)
        .map_err(|reason| format!("小梁 {}: {reason}", member.id.0))?;
    Ok(Some((sec, e, ft)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beam_normal_values_and_signed_forces() {
        let positive =
            design_beam_from_forces(4000.0, 10.0, 2.0e7, 2.0e4, 8.0, 1.0e6, 160.0, 250.0).unwrap();
        assert_eq!(positive.sigma, 20.0);
        assert_eq!(positive.bending_ratio, 0.125);
        assert_eq!(positive.deflection_span_ratio, 0.002);
        assert_eq!(positive.deflection_ratio, 0.5);
        assert_eq!(positive.ratio, 0.5);
        assert!(positive.ok);
        assert!(!positive.unchecked);
        let negative =
            design_beam_from_forces(4000.0, 10.0, -2.0e7, -2.0e4, -8.0, 1.0e6, 160.0, 250.0)
                .unwrap();
        assert_eq!(negative, positive);
    }

    #[test]
    fn beam_limit_is_inclusive_for_each_check() {
        for (moment, deflection, ratio, ok) in [
            (160.0e6, 0.0, 1.0, true),
            (0.0, 16.0, 1.0, true),
            (176.0e6, 0.0, 1.1, false),
            (0.0, 17.6, 1.1, false),
            (0.0, 0.0, 0.0, true),
        ] {
            let r =
                design_beam_from_forces(4000.0, 0.0, moment, 0.0, deflection, 1.0e6, 160.0, 250.0)
                    .unwrap();
            assert!((r.ratio - ratio).abs() < 1e-12);
            assert_eq!(r.ok, ok);
        }
    }

    #[test]
    fn beam_invalid_inputs_return_named_errors() {
        let names = [
            "スパン",
            "代表等分布荷重",
            "最大曲げモーメント",
            "最大せん断力",
            "たわみ",
            "断面係数",
            "許容曲げ応力度",
            "たわみ限界分母",
        ];
        for (index, name) in names.iter().enumerate() {
            let invalid = if [0, 5, 6, 7].contains(&index) {
                vec![0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY]
            } else {
                vec![f64::NAN, f64::INFINITY, f64::NEG_INFINITY]
            };
            for value in invalid {
                let mut args = [4000.0, 10.0, 2.0e7, 2.0e4, 10.0, 1.0e6, 160.0, 250.0];
                args[index] = value;
                let error = design_beam_from_forces(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7],
                )
                .unwrap_err();
                assert!(error.contains(name), "{error}");
            }
        }
    }

    #[test]
    fn beam_unrepresentable_ratios_return_errors() {
        for (moment, deflection, modulus, stress, span, limit) in [
            (f64::MAX, 0.0, f64::MIN_POSITIVE, 160.0, 4000.0, 250.0),
            (f64::MAX, 0.0, 1.0, f64::MIN_POSITIVE, 4000.0, 250.0),
            (0.0, f64::MAX, 1.0e6, 160.0, f64::MIN_POSITIVE, 250.0),
            (0.0, f64::MAX, 1.0e6, 160.0, 1.0, 250.0),
            (f64::MIN_POSITIVE, 0.0, f64::MAX, 160.0, 4000.0, 250.0),
            (1.0, 0.0, f64::MAX, f64::MAX, 4000.0, 250.0),
            (0.0, f64::MIN_POSITIVE, 1.0e6, 160.0, f64::MAX, 250.0),
            (0.0, 1.0, 1.0e6, 160.0, f64::MAX, f64::MIN_POSITIVE),
        ] {
            assert!(design_beam_from_forces(
                span, 0.0, moment, 0.0, deflection, modulus, stress, limit
            )
            .is_err());
        }
    }

    #[test]
    fn test_slab_oneway_moment_and_rebar() {
        // w=0.005 N/mm², L=3000mm, 単純支持(coef=8) → M=wL²/8=5625 N·mm/mm。
        let r = design_slab_oneway(
            3000.0,
            0.005,
            8.0,
            150.0,
            SLAB_DEFAULT_COVER,
            REBAR_FT_LONG_SD295,
            SLAB_J_RATIO,
        );
        assert!((r.moment - 0.005 * 3000.0 * 3000.0 / 8.0).abs() < 1e-6);
        assert!((r.effective_depth - 120.0).abs() < 1e-9);
        // As = M/(ft·j·d)。
        let expect_as = r.moment / (REBAR_FT_LONG_SD295 * SLAB_J_RATIO * 120.0);
        assert!((r.as_req_per_mm - expect_as).abs() / expect_as < 1e-9);
        assert!((r.as_req_per_m - expect_as * 1000.0).abs() / (expect_as * 1000.0) < 1e-9);
    }
}
