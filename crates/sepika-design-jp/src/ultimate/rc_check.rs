//! RC梁・矩形柱・円形柱の終局検定ドライバ。
//!
//! - [`UltimateCheck`] — 1 部材分の終局検定結果。
//! - [`collect_rc_ultimate_checks`] — モデルの RC梁・矩形柱・円形柱を一括検定する。

use crate::rc::{rc_member_clear_span, RcClearSpanError};
use crate::MemberKind;
use sepika_core::ids::ElemId;
use sepika_core::model::{ElementData, Material, Model, Section};
use sepika_core::rc_capacity::{rc_mu_simple, RcCapacityInput};
use sepika_core::section_shape::SectionShape;

use super::options::{
    MemberDemand, ShearMethod, StaticBeamBending, UltimateDemandMode, UltimateShearOptions,
};
use super::rc_axial::{rc_column_axial_ultimate, RcAxialUltimate};
use super::rc_props::{rc_bar_props, RcBarProps, RcDirection};
use super::rc_shear::{
    bond_reliable_strength_deformed, rc_shear_qbu_bond, BondStrengthInput, RcBondSplitInput,
};
use super::rc_shear_ductility::{rc_shear_vbu_ductility, RcVbuInput};
use super::rc_strength::{biaxial_margin, column_axis_shear, column_mu, member_shear_strength};

/// 1 部材分の終局検定結果。未検定で算定条件を確認できない値は NaN とする。
#[derive(Clone, Debug)]
pub struct UltimateCheck {
    /// 部材 ID。
    pub elem: ElemId,
    /// 部材種別（梁/柱）。
    pub kind: MemberKind,
    /// 曲げ終局強度 Mu [N·mm]。
    pub mu: f64,
    /// 比較用せん断需要 Qmu `[N]`。静的は上限強度倍率·2·Mu/内法、確定総応答は総せん断力の絶対値。
    pub qmu: f64,
    /// 選択された [`ShearMethod`] に応じた終局せん断強度 Qsu/Vu `[N]`。
    pub qsu: f64,
    /// 付着割裂による終局せん断耐力 Qbu `[N]`（`include_bond=false` なら 0）。
    pub qbu: f64,
    /// 強軸のせん断余裕度。静的は max(Qsu−|QL|, 0)/Qmu、確定総応答は Qsu/Qmu。
    /// Qmu が 0 の場合は無限大。ただし静的で |QL| が Qsu を超える場合は 0。
    pub shear_margin: f64,
    /// 二軸せん断余裕度。静的では両軸の長期せん断が明示的に 0、確定総応答では両軸の総せん断を使用できる場合に算定。
    pub biaxial_shear_margin: Option<f64>,
    /// 2 軸曲げ余裕度（柱かつ `biaxial_bending=true` のとき Some）。
    /// `1/((Mmx/Mux)^2+(Mmy/Muy)^2)^(1/2)`。設計用曲げ需要が 0 なら `f64::INFINITY`。
    pub biaxial_bending_margin: Option<f64>,
    /// 付着余裕度。静的は max(Qbu−|QL|, 0)/Qmu、確定総応答は Qbu/Qmu。
    /// `include_bond=false` または Qmu が 0 の場合は無限大。
    /// 付着が有効かつ静的で、Qmu が 0 でも |QL| が Qbu を超える場合は 0。
    pub bond_margin: f64,
    /// 軸終局耐力（柱のみ Some）。
    pub axial: Option<RcAxialUltimate>,
    /// 有効にした照査がすべて成立した場合だけ true。
    pub ok: bool,
    /// 部材・重力用途・組合せを付記した未検定理由。採用した方式で NG の部材では `None`。
    pub unchecked_reason: Option<String>,
    /// 根拠（表示用）。
    pub basis: String,
    /// 詳細（表示用）。
    pub detail: String,
}

/// `mz > 0` は下端引張、`mz < 0` は上端引張。柱と確定総応答の `mz == 0` は
/// 上下の引張鉄筋量 `at` が小さい側を使う。
fn strong_tension_is_top(shape: &SectionShape, mz: f64, need_be: bool) -> bool {
    if mz < 0.0 {
        return true;
    }
    if mz > 0.0 {
        return false;
    }
    let at_top = rc_bar_props(shape, RcDirection::Strong, true, need_be).map(|p| p.at);
    let at_bottom = rc_bar_props(shape, RcDirection::Strong, false, need_be).map(|p| p.at);
    match (at_top, at_bottom) {
        (Some(t), Some(b)) => t < b,
        (Some(_), None) => true,
        _ => false,
    }
}

fn unchecked_member(elem: ElemId, kind: MemberKind, reason: &str) -> UltimateCheck {
    UltimateCheck {
        elem,
        kind,
        mu: f64::NAN,
        qmu: f64::NAN,
        qsu: f64::NAN,
        qbu: f64::NAN,
        shear_margin: f64::NAN,
        biaxial_shear_margin: None,
        biaxial_bending_margin: None,
        bond_margin: f64::NAN,
        axial: None,
        ok: false,
        unchecked_reason: Some(reason.into()),
        basis: match kind {
            MemberKind::Column => "RC柱 終局検定",
            _ => "RC梁 終局検定",
        }
        .into(),
        detail: reason.into(),
    }
}

/// 1 部材の終局検定を実行する（対応断面以外は `Ok(None)`、
/// RC梁・矩形柱・円形柱なのに Fc・主筋降伏強度・配筋・有効せいが不整合なら部材 ID 付きで `Err`、
/// せん断補強筋に未対応グレードまたは `fy` 未設定がある場合も `Err`）。
fn check_member(
    elem: &ElementData,
    sec: &Section,
    mat: &Material,
    model: &Model,
    demand: MemberDemand,
    opts: &UltimateShearOptions,
) -> Result<Option<UltimateCheck>, String> {
    let Some(shape) = sec.shape.as_ref() else {
        return Ok(None);
    };
    let (b, d, kind) = match shape {
        SectionShape::RcBeamRect { b, d, .. } => (*b, *d, MemberKind::Girder),
        SectionShape::RcColumnRect { b, d, .. } => (*b, *d, MemberKind::Column),
        SectionShape::RcColumnCircle { d, .. } => (*d, *d, MemberKind::Column),
        _ => return Ok(None),
    };
    let Some(fc) = mat.fc else {
        return Err(format!(
            "部材 ID {} のコンクリート強度 Fc が未設定です",
            elem.id.0
        ));
    };
    if fc <= 0.0 || b <= 0.0 || d <= 0.0 {
        return Err(format!(
            "部材 ID {} の Fc または断面寸法が不正です",
            elem.id.0
        ));
    }
    if let Err(reason) = shape.validate_rebar_size_materials(
        model.element_rebar_material(elem),
        model.element_shear_rebar_material(elem),
    ) {
        return Err(format!("部材 ID {} の{reason}", elem.id.0));
    }
    if let Some(msg) = sepika_core::material_grade::shear_rebar_material_issue(
        model.element_shear_rebar_material(elem),
    ) {
        return Err(format!("部材 ID {} の{}", elem.id.0, msg));
    }
    let opts_owned = UltimateShearOptions {
        rp: demand.rp.map(|rp| rp.max(0.0)).unwrap_or(opts.rp),
        sigma_wy: sepika_core::material_grade::shear_rebar_yield_strength(
            model.element_shear_rebar_material(elem),
        )
        .unwrap_or(opts.sigma_wy),
        ..opts.clone()
    };
    let opts = &opts_owned;
    let Some(sigma_y) =
        sepika_core::material_grade::rebar_yield_strength(model.element_rebar_material(elem))
    else {
        return Err(format!(
            "部材 ID {} の主筋の降伏強度を解決できません",
            elem.id.0
        ));
    };
    let invalid_rebar = || {
        format!(
            "部材 ID {} の配筋形状が不正または有効せいが 0 以下です",
            elem.id.0
        )
    };
    let static_beam_rebar = match (shape, demand.mode) {
        (SectionShape::RcBeamRect { rebar, .. }, UltimateDemandMode::StaticHinge) => Some(rebar),
        _ => None,
    };
    let l_clear = rc_member_clear_span(elem, model).map_err(|error| {
        let code = match error {
            RcClearSpanError::Uncomputed(_) => "ultimate_clear_span_uncomputed",
            RcClearSpanError::Invalid(_) => "ultimate_clear_span_invalid",
        };
        format!("{code}: {error}")
    });
    if let Some(rebar) = static_beam_rebar {
        let reason = match demand.static_beam_bending {
            StaticBeamBending::Mixed => Some("ultimate_beam_mz_mixed_signs: 強軸曲げに正負が混在し、両端に適用する引張側を決定できません"),
            StaticBeamBending::Unverified => Some("ultimate_beam_mz_unverified: 強軸曲げの符号を確認できず、引張側を決定できません"),
            StaticBeamBending::Signed if demand.mz == 0.0 => Some("ultimate_beam_mz_zero: 強軸曲げが0で、引張側を決定できません"),
            StaticBeamBending::Signed => None,
        };
        if let Some(reason) = reason {
            if rebar.is_unset() || rebar.validate(b, d).is_err() {
                return Err(invalid_rebar());
            }
            let reason = l_clear.as_ref().err().map_or(reason, String::as_str);
            return Ok(Some(unchecked_member(elem.id, kind, reason)));
        }
    }

    let need_be = opts.shear_method == ShearMethod::Ductility;
    let tension_is_top = strong_tension_is_top(shape, demand.mz, need_be);
    let p = rc_bar_props(shape, RcDirection::Strong, tension_is_top, need_be)
        .ok_or_else(invalid_rebar)?;
    let l_clear = match l_clear {
        Ok(span) => span,
        Err(reason) => return Ok(Some(unchecked_member(elem.id, kind, &reason))),
    };
    if static_beam_rebar.is_some() && p.at <= 0.0 {
        let reason = if tension_is_top {
            "ultimate_beam_tension_rebar_missing: 上端引張の主筋がありません"
        } else {
            "ultimate_beam_tension_rebar_missing: 下端引張の主筋がありません"
        };
        return Ok(Some(unchecked_member(elem.id, kind, reason)));
    }
    let jt = 7.0 * p.d_eff / 8.0;
    let at = p.at;
    let ag = p.ag;
    let pw = p.pw;
    let n_axial = demand.n_axial;

    let cap = RcCapacityInput {
        b: p.b_dir,
        d: p.d_dir,
        at,
        d_eff: p.d_eff,
        sigma_y,
        fc,
        pw,
        sigma_wy: opts.sigma_wy,
        clear_span: l_clear,
        sigma_0: 0.0,
    };
    let mu = match kind {
        MemberKind::Column => column_mu(p.b_dir, p.d_dir, p.dt, at, ag, sigma_y, fc, n_axial),
        _ => rc_mu_simple(&cap),
    };

    let qmu = match demand.shear {
        Some(qm) => opts.upper_strength_factor * qm.abs(),
        None => opts.upper_strength_factor * 2.0 * mu / l_clear,
    };

    let qsu = member_shear_strength(&p, fc, n_axial, l_clear, opts);

    let bond_strength = |p: &RcBarProps| {
        let jt = 7.0 * p.d_eff / 8.0;
        let tau_bu = bond_reliable_strength_deformed(&BondStrengthInput {
            fc,
            b: p.b_dir,
            db1: p.main_dia.designation_or_diameter_mm(),
            n_bars: p.n_tension,
            cover_side: p.cover,
            cover_bottom: p.cover,
            hoop_area: p.shear_legs as f64 * p.shear_dia.area_mm2(),
            hoop_pitch: p.shear_pitch,
            pw: p.pw,
            top_bar: p.top_bar,
        });
        let sum_phi = p.n_tension as f64 * p.main_dia.perimeter_mm();
        let qbu = match opts.shear_method {
            ShearMethod::Plastic => rc_shear_qbu_bond(&RcBondSplitInput {
                b: p.b_dir,
                d_full: p.d_dir,
                jt,
                tau_bu,
                sum_phi,
                l_clear,
                fc,
                rp: opts.rp,
                lightweight: opts.lightweight,
            }),
            ShearMethod::Ductility => rc_shear_vbu_ductility(&RcVbuInput {
                b: p.b_dir,
                d_full: p.d_dir,
                be: p.be,
                je: jt,
                tau_bu,
                sum_phi1: sum_phi,
                tau_bu2: 0.0,
                sum_phi2: 0.0,
                s: p.shear_pitch,
                n_s: p.n_s,
                l_clear,
                fc,
                rp: opts.rp,
                tensile_axial: n_axial < 0.0,
                yield_hinge: opts.rp > 0.0,
                lightweight: opts.lightweight,
            }),
        };
        (qbu, tau_bu)
    };
    let (qbu, tau_bu) = if opts.include_bond {
        bond_strength(&p)
    } else {
        (0.0, 0.0)
    };

    let biaxial_shear_requested = kind == MemberKind::Column && opts.biaxial_shear;
    let static_biaxial_shear =
        biaxial_shear_requested && demand.mode == UltimateDemandMode::StaticHinge;
    let ql = demand.q_long.map(|q| q.abs()).unwrap_or(0.0);
    let gravity_exceeds_capacity = |capacity_n: f64| ql > capacity_n;
    let margin = |capacity_n: f64| {
        if demand.mode == UltimateDemandMode::StaticHinge && gravity_exceeds_capacity(capacity_n) {
            0.0
        } else if qmu > 0.0 {
            (capacity_n - ql).max(0.0) / qmu
        } else {
            f64::INFINITY
        }
    };
    let shear_margin = margin(qsu);
    let bond_margin = if opts.include_bond {
        margin(qbu)
    } else {
        f64::INFINITY
    };

    let biaxial_shear = biaxial_shear_requested.then(|| {
        if static_biaxial_shear {
            let ql_weak = demand.q_long_weak.ok_or(
                "ultimate_gravity_missing: 静的二軸せん断に必要な弱軸の長期せん断力がありません",
            )?;
            if ql != 0.0 || ql_weak != 0.0 {
                return Err("長期せん断力がある RC 柱の静的二軸せん断は適用範囲外です");
            }
        }
        let p_y = rc_bar_props(shape, RcDirection::Weak, tension_is_top, need_be)
            .ok_or("二軸せん断に必要な弱軸の配筋形状または有効せいが不正です")?;
        let (qsu_y, qmu_y_hinge) = column_axis_shear(&p_y, fc, sigma_y, n_axial, l_clear, opts);
        let qmu_y = match demand.shear_weak {
            Some(qmy) => opts.upper_strength_factor * qmy.abs(),
            None => qmu_y_hinge,
        };
        let rx = if qsu > 0.0 { qmu / qsu } else { f64::INFINITY };
        let ry = if qsu_y > 0.0 {
            qmu_y / qsu_y
        } else {
            f64::INFINITY
        };
        Ok(biaxial_margin(rx, ry, 2.0))
    });
    let biaxial_shear_margin = biaxial_shear.and_then(Result::ok);

    let biaxial_bending_margin = if kind == MemberKind::Column && opts.biaxial_bending {
        rc_bar_props(shape, RcDirection::Weak, tension_is_top, need_be).map(|p_y| {
            let mux = mu;
            let muy = column_mu(
                p_y.b_dir, p_y.d_dir, p_y.dt, p_y.at, p_y.ag, sigma_y, fc, n_axial,
            );
            let rx = if mux > 0.0 {
                demand.mz.abs() / mux
            } else if demand.mz.abs() > 0.0 {
                f64::INFINITY
            } else {
                0.0
            };
            let ry = if muy > 0.0 {
                demand.my.abs() / muy
            } else if demand.my.abs() > 0.0 {
                f64::INFINITY
            } else {
                0.0
            };
            biaxial_margin(rx, ry, 2.0)
        })
    } else {
        None
    };

    let axial = if kind == MemberKind::Column {
        Some(rc_column_axial_ultimate(p.b_dir, p.d_dir, fc, ag, sigma_y))
    } else {
        None
    };

    let shear_ok = shear_margin >= 1.0 && biaxial_shear_margin.is_none_or(|m| m >= 1.0);
    let bending_ok = biaxial_bending_margin.map(|m| m >= 1.0).unwrap_or(true);
    let checked_parts_ok = shear_ok && bond_margin >= 1.0 && bending_ok;
    if checked_parts_ok && static_beam_rebar.is_some() {
        let insufficient_capacity =
            |capacity_n| gravity_exceeds_capacity(capacity_n) || (capacity_n - ql).max(0.0) < qmu;
        let opposite = rc_bar_props(shape, RcDirection::Strong, !tension_is_top, need_be)
            .filter(|props| props.at > 0.0);
        let reason = if let Some(opposite) = opposite {
            let opposite_mu = rc_mu_simple(&RcCapacityInput {
                at: opposite.at,
                d_eff: opposite.d_eff,
                ..cap
            });
            let opposite_qsu = member_shear_strength(&opposite, fc, n_axial, l_clear, opts);
            if opposite_mu > mu {
                Some("ultimate_beam_hinge_bending_unverified: 反対側の曲げ終局耐力が大きく、同じ耐力を両端に用いる略算の適用を確認できません")
            } else if insufficient_capacity(opposite_qsu) {
                Some("ultimate_beam_hinge_shear_unverified: 反対側のせん断耐力で比較需要を満たせず、両端ヒンジ略算の適用を確認できません")
            } else if opts.include_bond && insufficient_capacity(bond_strength(&opposite).0) {
                Some("ultimate_beam_hinge_bond_unverified: 反対側の付着耐力で比較需要を満たせず、両端ヒンジ略算の適用を確認できません")
            } else {
                None
            }
        } else {
            Some("ultimate_beam_hinge_rebar_missing: 反対側の引張主筋の諸元が不足し、両端ヒンジ略算の適用を確認できません")
        };
        if let Some(reason) = reason {
            return Ok(Some(UltimateCheck {
                mu,
                qsu,
                qbu,
                detail: format!("Mu={mu:.0} N·mm, Qsu={qsu:.0} N, Qbu={qbu:.0} N。{reason}"),
                ..unchecked_member(elem.id, kind, reason)
            }));
        }
    }
    let unchecked_reason = if checked_parts_ok {
        biaxial_shear.and_then(Result::err).map(str::to_owned)
    } else {
        None
    };
    let ok = checked_parts_ok && unchecked_reason.is_none();

    let shear_label = match opts.shear_method {
        ShearMethod::Plastic => "塑性理論式 Qsu",
        ShearMethod::Ductility => "靭性指針式 Vu",
    };
    let basis = match kind {
        MemberKind::Column => format!("RC柱 終局検定（{shear_label}/Qbu）"),
        _ => format!("RC梁 終局検定（{shear_label}/Qbu）"),
    };
    let biaxial_str = match biaxial_shear_margin {
        Some(m) => format!(", 2軸せん断余裕度={m:.3}"),
        None => String::new(),
    };
    let ql_str = if static_biaxial_shear {
        let weak = demand
            .q_long_weak
            .map_or_else(|| "未取得".into(), |q| q.abs().to_string());
        format!(", 長期せん断[N] 強軸={ql}, 弱軸={weak}")
    } else {
        String::new()
    };
    let bend_str = match biaxial_bending_margin {
        Some(m) => format!(", 2軸曲げ余裕度={m:.3}"),
        None => String::new(),
    };
    let detail = format!(
        "Mu={:.0} N·mm, Qmu={:.0} N, Qsu={:.0} N, Qbu={:.0} N, τbu={:.3} N/mm², \
         Qsu/Qmu={:.3}, Qbu/Qmu={:.3}{}{}{}, pw={:.5}, jt={:.1} mm, L={} mm, Rp={:.4}",
        mu,
        qmu,
        qsu,
        qbu,
        tau_bu,
        shear_margin,
        bond_margin,
        biaxial_str,
        bend_str,
        ql_str,
        pw,
        jt,
        l_clear,
        opts.rp
    );

    Ok(Some(UltimateCheck {
        elem: elem.id,
        kind,
        mu,
        qmu,
        qsu,
        qbu,
        shear_margin,
        biaxial_shear_margin,
        biaxial_bending_margin,
        bond_margin,
        axial,
        ok,
        unchecked_reason,
        basis,
        detail,
    }))
}

/// RC梁・柱の終局検定。必須需要・材料・配筋・寸法の不足は部材ID付きのエラーを返す。
/// 上限強度倍率は静的・両端ヒンジ略算で有限の正値、確定総応答で 1.0 を要し、違反時は部材ID付きのエラーを返す。
/// 内法長さを確認できない部材や、静的RC梁の引張側・主筋や二軸せん断の適用条件が不足して検定できない部材は、未検定理由を保持する。
pub fn collect_rc_ultimate_checks(
    model: &Model,
    demand_by_elem: &[(ElemId, MemberDemand)],
    opts: &UltimateShearOptions,
) -> Result<Vec<UltimateCheck>, String> {
    let mut out = Vec::new();
    for elem in &model.elements {
        let Some(sec) = elem.section.and_then(|sid| model.sections.get(sid.index())) else {
            continue;
        };
        let is_rc = matches!(
            sec.shape.as_ref(),
            Some(
                SectionShape::RcBeamRect { .. }
                    | SectionShape::RcColumnRect { .. }
                    | SectionShape::RcColumnCircle { .. }
            )
        );
        if is_rc {
            model.ensure_stb_rebar_supported(sec.id)?;
        }
        let Some(mat) = model.element_material(elem) else {
            if is_rc {
                return Err(format!("部材 ID {} の材料が未設定です", elem.id.0));
            }
            continue;
        };
        if !is_rc {
            continue;
        }
        let diagnose = |reason| super::demand_diagnostic::demand_diagnostic(model, elem.id, reason);
        sepika_core::load_combo::ultimate_gravity_terms(model)
            .map_err(|reason| diagnose(format!("RC部材 ID {}: {reason}", elem.id.0)))?;
        let mut demands = demand_by_elem.iter().filter(|(id, _)| *id == elem.id);
        let demand = demands.next().map(|(_, d)| *d).ok_or_else(|| {
            diagnose(format!(
                "ultimate_demand_missing: RC部材 ID {} の長期架構用需要が不足しています",
                elem.id.0
            ))
        })?;
        if demands.next().is_some() {
            return Err(diagnose(format!(
                "ultimate_demand_duplicate: RC部材 ID {} の需要が重複しています",
                elem.id.0
            )));
        }
        if [demand.n_axial, demand.mz, demand.my]
            .iter()
            .any(|v| !v.is_finite())
            || [
                demand.shear,
                demand.shear_weak,
                demand.rp,
                demand.q_long,
                demand.q_long_weak,
            ]
            .into_iter()
            .flatten()
            .any(|v| !v.is_finite())
        {
            return Err(diagnose(format!(
                "ultimate_demand_nonfinite: RC部材 ID {} の需要が非有限です",
                elem.id.0
            )));
        }
        match demand.mode {
            UltimateDemandMode::StaticHinge
                if demand.q_long.is_none()
                    || demand.shear.is_some()
                    || demand.shear_weak.is_some() =>
            {
                return Err(diagnose(format!(
                    "ultimate_gravity_missing: RC部材 ID {} の静的N/M/QL参照が不完全です",
                    elem.id.0
                )));
            }
            UltimateDemandMode::StaticHinge
                if !opts.upper_strength_factor.is_finite() || opts.upper_strength_factor <= 0.0 =>
            {
                return Err(diagnose(format!(
                    "ultimate_upper_strength_factor_invalid: RC部材 ID {} の上限強度倍率は有限かつ正である必要があります（入力値: {}）",
                    elem.id.0, opts.upper_strength_factor
                )));
            }
            UltimateDemandMode::ConfirmedTotal
                if demand.shear.is_none()
                    || demand.shear_weak.is_none()
                    || demand.rp.is_none()
                    || demand.q_long.is_some()
                    || demand.q_long_weak.is_some()
                    || opts.upper_strength_factor != 1.0 =>
            {
                return Err(diagnose(format!("ultimate_total_unsupported: RC部材 ID {} の確定総応答は追加QLなし・倍率1の完全な応答が必要です", elem.id.0)));
            }
            _ => {}
        }
        if let Some(mut check) = check_member(elem, sec, mat, model, demand, opts)? {
            check.unchecked_reason = check.unchecked_reason.map(diagnose);
            check.detail = format!(
                "mode={:?}, gravity_terms={:?}; {}",
                demand.mode,
                sepika_core::load_combo::ultimate_gravity_terms(model)?,
                check.detail
            );
            out.push(check);
        }
    }
    Ok(out)
}
