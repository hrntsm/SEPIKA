//! RC梁・矩形柱・円形柱の終局検定ドライバ。
//!
//! - [`UltimateCheck`] — 1 部材分の終局検定結果。
//! - [`collect_rc_ultimate_checks`] — モデルの RC梁・矩形柱・円形柱を一括検定する。

use std::collections::HashMap;

use num_rational::BigRational;
use num_traits::{One, ToPrimitive, Zero};

use crate::material_strength::{main_rebar_grade, shear_rebar_category_issue};
use crate::member_design_check::combine_incomplete_reasons;
use crate::rc::{rc_member_clear_span, RcClearSpanError};
use crate::MemberKind;
use sepika_core::dof::{Dof, Dof6Mask};
use sepika_core::ids::{ElemId, NodeId};
use sepika_core::material_grade::{rebar_f_value, MAIN_REBAR_GRADES};
use sepika_core::model::{
    Constraint, ElementData, ElementKind, EndCondition, Material, Model, Section,
};
use sepika_core::rc_capacity::{rc_mu_simple, RcCapacityInput};
use sepika_core::section_shape::{RebarSize, SectionShape};

use super::options::{
    MemberDemand, ShearMethod, StaticBeamBending, StaticBeamHingeMechanism, UltimateDemandMode,
    UltimateShearOptions,
};
use super::rc_axial::{rc_column_axial_ultimate, RcAxialUltimate};
use super::rc_props::{rc_bar_props, RcBarProps, RcDirection};
use super::rc_shear::{
    bond_reliable_strength_deformed, rc_shear_qbu_bond, BondStrengthInput, RcBondSplitInput,
};
use super::rc_shear_ductility::{
    ductility_nu, ductility_truss_upper_bound, rc_shear_vbu_ductility, truss_lambda, RcVbuInput,
};
use super::rc_strength::{biaxial_margin, column_mu, member_shear_strength};

const BEAM_SHEAR_UPPER_BOUND_UNVERIFIED: &str =
    "ultimate_beam_hinge_shear_upper_bound_unverified: 保証設計の要求せん断力を過小評価しない曲げ耐力側の強度・係数を確認できません";

/// 1 部材分の終局検定結果。未検定で算定条件を確認できない値は NaN とする。
#[derive(Clone, Debug)]
pub struct UltimateCheck {
    /// 部材 ID。
    pub elem: ElemId,
    /// 部材種別（梁/柱）。
    pub kind: MemberKind,
    /// 曲げ終局強度 Mu [N·mm]。RC梁は適用未確認の参考値で、方向と理由は `detail` に示す。
    pub mu: f64,
    /// 比較用せん断需要 [N]。静的RC柱は Qmu（上限強度倍率·2·Mu/内法）、確定総応答は |Qtotal|。
    /// 静的RC梁および数値の表現範囲を超える静的RC柱の需要は未算定のため NaN。
    pub qmu: f64,
    /// 選択された [`ShearMethod`] に応じた終局せん断強度 Qsu/Vu `[N]`。
    pub qsu: f64,
    /// 選択された方式に応じた付着耐力 Qbu/Vbu `[N]`（`include_bond=false` なら 0）。
    pub qbu: f64,
    /// 強軸のせん断余裕度。静的RC柱は長期せん断を控除し、確定総応答は耐力を直接比較する。
    /// 比較需要が 0 の場合は無限大。ただし静的RC柱で長期せん断力が耐力を超える場合は 0。
    /// 静的RC梁は長期せん断力だけで超過が確定した場合は 0、それ以外は NaN。
    pub shear_margin: f64,
    /// 二軸せん断余裕度。静的では両軸の長期せん断が明示的に 0、確定総応答では両軸の総せん断を使用できる場合に算定。
    pub biaxial_shear_margin: Option<f64>,
    /// 2 軸曲げ余裕度（柱かつ `biaxial_bending=true` のとき Some）。
    /// `1/((Mmx/Mux)^2+(Mmy/Muy)^2)^(1/2)`。設計用曲げ需要が 0 なら `f64::INFINITY`。
    pub biaxial_bending_margin: Option<f64>,
    /// 付着余裕度。静的RC柱は長期せん断を控除し、確定総応答は耐力を直接比較する。
    /// 付着が無効、または耐力が算定済みで比較需要が 0 なら無限大。
    /// 静的RC柱で付着が有効かつ長期せん断力が耐力を超える場合は 0、耐力が未算定の場合は NaN。
    /// 静的RC梁は付着が有効で長期せん断力だけで超過が確定した場合は 0、それ以外は NaN。
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

fn static_hinge_shear(mu_n_mm: f64, factor: f64, span_mm: f64, axis: &str) -> Result<f64, String> {
    if mu_n_mm == 0.0 {
        return Ok(0.0);
    }
    if !mu_n_mm.is_finite() {
        return Err(format!(
            "ultimate_hinge_shear_mu_nonfinite: {axis}の曲げ終局耐力 Mu が非有限のためヒンジせん断需要を算定できません"
        ));
    }
    // 積と除算の順を変えるだけでは、最終値が有限でも中間値が桁あふれや丸めで失われる。
    let (mu_fraction, mu_exponent) = libm::frexp(mu_n_mm);
    let (factor_fraction, factor_exponent) = libm::frexp(factor);
    let (span_fraction, span_exponent) = libm::frexp(span_mm);
    let estimate_n = libm::scalbn(
        mu_fraction * factor_fraction / span_fraction,
        1 + mu_exponent + factor_exponent - span_exponent,
    );
    let shear_n = if (2.0 * f64::MIN_POSITIVE..f64::MAX / 2.0).contains(&estimate_n) {
        estimate_n
    } else {
        // 仮数の丸めが最終の丸めに重なると 0 や inf を誤判定しうる。正規数の上下端に接する
        // 指数帯も含め、有理数のまま求めて最後に丸める。
        let mu = BigRational::from_float(mu_n_mm).unwrap();
        let factor = BigRational::from_float(factor).unwrap();
        let span = BigRational::from_float(span_mm).unwrap();
        ((&mu + &mu) * factor / span).to_f64().unwrap()
    };
    if !shear_n.is_finite() {
        Err(format!(
            "ultimate_hinge_shear_overflow: {axis}のヒンジせん断需要が数値の表現上限を超えます"
        ))
    } else if shear_n == 0.0 {
        Err(format!(
            "ultimate_hinge_shear_underflow: {axis}の正のヒンジせん断需要が数値の表現範囲を下回り、丸めで 0 N になります"
        ))
    } else {
        Ok(shear_n)
    }
}

fn static_biaxial_margin(
    (strong_demand_n, strong_capacity_n): (f64, f64),
    (weak_demand_n, weak_capacity_n): (f64, f64),
    shear_rebar_unknown: bool,
) -> f64 {
    let axis_margin = |demand_n: f64, capacity_n: f64| {
        if capacity_n > 0.0 {
            capacity_n / demand_n
        } else if shear_rebar_unknown && capacity_n == 0.0 && demand_n == 0.0 {
            f64::INFINITY
        } else {
            0.0
        }
    };
    let strong_margin = axis_margin(strong_demand_n, strong_capacity_n);
    let weak_margin = axis_margin(weak_demand_n, weak_capacity_n);
    let min = strong_margin.min(weak_margin);
    let max = strong_margin.max(weak_margin);
    if min == 0.0 {
        return min;
    }
    // 需要／耐力やそのノルムの上限を超えても、逆数の余裕度は表現できる場合がある。
    if max.is_finite() {
        return min / 1.0_f64.hypot(min / max);
    }

    let scale = if min.is_finite() {
        min
    } else {
        2.0_f64.powi(1023)
    };
    let scaled_demand_ratio = |demand_n: f64, capacity_n: f64, margin: f64| {
        // 有限かつ正の耐力との比があふれる需要は 1 N 未満なので、需要を先に拡大できる。
        if margin.is_infinite() && capacity_n > 0.0 && capacity_n.is_finite() {
            (scale * demand_n) / capacity_n
        } else {
            scale / margin
        }
    };
    let strong_ratio = scaled_demand_ratio(strong_demand_n, strong_capacity_n, strong_margin);
    let weak_ratio = scaled_demand_ratio(weak_demand_n, weak_capacity_n, weak_margin);
    let norm = strong_ratio.hypot(weak_ratio);
    let estimate = scale / norm;
    if min.is_infinite() && (norm - 0.5).abs() <= 4.0 * f64::EPSILON {
        // 比とノルムの丸めが重なると、有限値と無限大の境界を越えうる。
        // 上側の丸め境界 MAX + 半 ULP に対応するノルムはほぼ 0.5。平方根を取ると
        // また丸めが入るため、二乗のまま境界と比較する。
        let squared_ratio = |demand_n: f64, capacity_n: f64| {
            if capacity_n > 0.0 && capacity_n.is_finite() {
                let demand = BigRational::from_float(demand_n).unwrap();
                let capacity = BigRational::from_float(capacity_n).unwrap();
                (demand / capacity).pow(2)
            } else {
                BigRational::zero()
            }
        };
        let boundary = BigRational::from_float(f64::MAX).unwrap()
            + BigRational::from_float(2.0_f64.powi(970)).unwrap();
        let sum = squared_ratio(strong_demand_n, strong_capacity_n)
            + squared_ratio(weak_demand_n, weak_capacity_n);
        if sum * boundary.pow(2) <= BigRational::one() {
            return f64::INFINITY;
        }
        return estimate.min(f64::MAX);
    }
    estimate
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

fn node_connection_counts(model: &Model) -> HashMap<NodeId, usize> {
    let mut counts = HashMap::new();
    let mut add = |id: NodeId| {
        *counts.entry(id).or_insert(0) += 1;
    };
    for node in model.elements.iter().flat_map(|elem| &elem.nodes) {
        add(*node);
    }
    // 解析用の壁要素が未展開のモデルでも、壁版の取り付く節点を自由端に分類しない。
    for node in model
        .wall_plates
        .iter()
        .filter_map(|plate| plate.boundary_nodes(model))
        .flatten()
    {
        add(node);
    }
    for constraint in &model.constraints {
        match constraint {
            Constraint::RigidDiaphragm { master, slaves, .. }
            | Constraint::RigidLink { master, slaves, .. } => {
                add(*master);
                slaves.iter().copied().for_each(&mut add);
            }
            Constraint::Mpc { master, terms, .. } => {
                add(*master);
                terms.iter().for_each(|(node, _, _)| add(*node));
            }
        }
    }
    counts
}

fn static_beam_support_reason(
    elem: &ElementData,
    model: &Model,
    connection_counts: &HashMap<NodeId, usize>,
) -> Option<String> {
    if !matches!(
        elem.kind,
        ElementKind::Beam | ElementKind::Fiber | ElementKind::MultiSpring
    ) {
        return Some(
            "ultimate_beam_hinge_support_unverified: 強軸曲げを伝達する梁要素を確認できません"
                .into(),
        );
    }
    let rotational_dofs = [Dof::Rx, Dof::Ry, Dof::Rz];
    let mut unverified_end = None;
    for (index, end) in ["i端", "j端"].into_iter().enumerate() {
        let condition = elem.end_cond[index];
        if matches!(
            condition,
            EndCondition::Pinned | EndCondition::SemiRigid { k_theta: 0.0 }
        ) {
            return Some(format!(
                "ultimate_beam_hinge_end_released: {end}の強軸曲げを解放しているため、両端ヒンジ略算の適用外です"
            ));
        }
        let Some(node) = elem.nodes.get(index).and_then(|id| model.node(*id)) else {
            unverified_end.get_or_insert(end);
            continue;
        };
        if condition == EndCondition::Fixed
            && rotational_dofs
                .into_iter()
                .all(|dof| node.restraint.is_fixed(dof))
        {
            continue;
        }
        let unattached = connection_counts.get(&node.id) == Some(&1);
        let all_rotations_free = rotational_dofs
            .into_iter()
            .all(|dof| !node.restraint.is_fixed(dof))
            && node
                .support_spring
                .is_none_or(|spring| spring[3..].iter().all(|k| *k == 0.0));
        let free = node.restraint == Dof6Mask::FREE
            && node.support_spring.is_none_or(|spring| spring == [0.0; 6]);
        if unattached && (free || (all_rotations_free && elem.end_offsets_mm[index] == [0.0; 3])) {
            let (code, support) = if free {
                ("ultimate_beam_hinge_free_end", "自由端")
            } else {
                ("ultimate_beam_hinge_end_released", "回転自由な支点")
            };
            return Some(format!(
                "{code}: {end}が{support}で強軸曲げを支持先に伝達できないため、両端ヒンジ略算の適用外です"
            ));
        }
        unverified_end.get_or_insert(end);
    }
    unverified_end.map(|end| {
        format!("ultimate_beam_hinge_support_unverified: {end}の支持・接合による強軸曲げ伝達を確認できません")
    })
}

fn beam_mu_direction_detail(p: &RcBarProps, location: &str) -> String {
    let side = if p.top_bar {
        "上端引張"
    } else {
        "下端引張"
    };
    if p.at <= 0.0 {
        return format!("{location}{side}: 引張主筋がありません");
    }
    let pt = 100.0 * (p.at / p.b_dir) / p.d_eff;
    let ratio = if pt >= 2.0 {
        "、表示定義で2%以上です"
    } else {
        ""
    };
    format!(
        "{location}{side}: a_t/(b d)={pt:.4}%{ratio}、圧縮幅/有効せい={:.3}",
        p.b_dir / p.d_eff
    )
}

fn beam_mu_applicability_reason(
    mat: &Material,
    fc: f64,
    main_rebar: &Material,
    sigma_y: f64,
    demand: MemberDemand,
) -> String {
    let mut details = vec![format!(
        "Fc={fc} N/mm²、主筋 {}（fy={sigma_y} N/mm²）",
        main_rebar.name
    )];
    if fc < 18.0 {
        details.push("Fcが通常域の目安18 N/mm²を下回ります".into());
    }
    let grade = main_rebar.name.trim();
    if sigma_y > 490.0 {
        details.push("主筋強度が原典記載のSD490級を超えます".into());
    } else if !MAIN_REBAR_GRADES.contains(&grade) || rebar_f_value(grade) != Some(sigma_y) {
        details.push("材料名とfyから原典の通常域に該当する主筋材種を確認できません".into());
    }
    if !mat.is_standard_concrete() {
        details.push("普通コンクリートを確認できません".into());
    }
    if demand.n_axial != 0.0 || demand.my != 0.0 {
        details.push("軸力または弱軸曲げを含む断面の略算適用を確認できません".into());
    }
    details.push(
        "圧縮幅の適否と、圧縮コンクリートの破壊に先行する引張主筋の降伏を確認できません".into(),
    );
    format!(
        "ultimate_beam_mu_applicability_unverified: {}",
        details.join("。")
    )
}

/// 1 部材の終局検定を実行する（対応断面以外は `Ok(None)`、
/// RC梁・矩形柱・円形柱なのに Fc・主筋降伏強度・配筋・有効せいが不整合なら部材 ID 付きで `Err`、
/// せん断補強筋の材種が未対応または Fy が未入力の場合も `Err`。区分の不備と明示の不正な Fy は結果に記録する）。
fn check_member(
    elem: &ElementData,
    sec: &Section,
    mat: &Material,
    model: &Model,
    connection_counts: &HashMap<NodeId, usize>,
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
    let (shear_symbol, bond_symbol) = opts.shear_method.capacity_symbols();
    let bond_detail = |margin_label: &str, qbu: f64, tau_bu: f64, margin: f64| {
        let display = |value: f64, decimals, unit| {
            if !opts.include_bond {
                "無効".into()
            } else if value.is_nan() {
                "未算定".into()
            } else {
                format!("{value:.decimals$}{unit}")
            }
        };
        let capacity = display(qbu, 0, " N");
        let strength = display(tau_bu, 3, " N/mm²");
        let margin = display(margin, 3, "");
        format!("{bond_symbol}={capacity}, τbu={strength}, {margin_label}={margin}")
    };
    let unavailable_bond_detail = || bond_detail("付着余裕度", f64::NAN, f64::NAN, f64::NAN);
    let unchecked_without_bond_values = |reason: &str| {
        let mut check = unchecked_member(elem.id, kind, reason);
        check
            .detail
            .push_str(&format!("。{}", unavailable_bond_detail()));
        check
    };
    let main_rebar = model.element_rebar_material(elem);
    let main_rebar_issue = crate::material_strength::main_rebar_category_issue(main_rebar)
        .map(|reason| format!("部材 ID {}（断面「{}」）の{reason}", elem.id.0, sec.name));
    let shear_rebar = model.element_shear_rebar_material(elem);
    let shear_category_issue = shear_rebar_category_issue(sec, shear_rebar)
        .map(|reason| format!("部材 ID {}: {reason}", elem.id.0));
    let shear_rebar = shear_rebar.filter(|_| shear_category_issue.is_none());
    let shear_rebar_issue = shear_category_issue.clone().or_else(|| {
        shear_rebar
            .and_then(sepika_core::material_grade::shear_rebar_fy_issue)
            .map(|reason| format!("部材 ID {}（断面「{}」）の{reason}", elem.id.0, sec.name))
    });
    let grade = main_rebar_grade(main_rebar);
    let round_bond_reason = (opts.include_bond
        && main_rebar_issue.is_none()
        && matches!(grade, "SR235" | "SR295"))
    .then(|| {
        format!(
            "ultimate_bond_round_bar_unsupported: 部材 ID {} の主筋は丸鋼（{grade}）です。終局付着割裂に用いる付着信頼強度の適用根拠を確認できないため、付着は未算定です",
            elem.id.0
        )
    });
    let unchecked = |reason: &str| {
        let reason = main_rebar_issue
            .as_ref()
            .map_or_else(|| reason.to_owned(), |issue| format!("{issue}。{reason}"));
        let reason = [shear_rebar_issue.as_deref(), round_bond_reason.as_deref()]
            .into_iter()
            .flatten()
            .fold(reason, |reason, issue| format!("{reason}。{issue}"));
        Ok(Some(unchecked_without_bond_values(&reason)))
    };
    let Some(fc) = mat.fc else {
        return Err(format!(
            "部材 ID {} のコンクリート強度 Fc が未設定です",
            elem.id.0
        ));
    };
    if [fc, b, d].into_iter().any(|v| !v.is_finite() || v <= 0.0) {
        return Err(format!(
            "部材 ID {} の Fc または断面寸法が不正です",
            elem.id.0
        ));
    }
    if let Err(reason) = shape.validate_rebar_size_materials(
        main_rebar.filter(|_| main_rebar_issue.is_none()),
        shear_rebar,
    ) {
        if main_rebar_issue.is_some() || shear_rebar_issue.is_some() {
            return unchecked(&reason);
        }
        return Err(format!("部材 ID {} の{reason}", elem.id.0));
    }
    if let Some(msg) = sepika_core::material_grade::shear_rebar_material_issue(
        shear_rebar.filter(|_| shear_rebar_issue.is_none()),
    ) {
        return Err(format!("部材 ID {} の{msg}", elem.id.0));
    }
    let rp = demand.rp.map_or(opts.rp, |rp| rp.max(0.0));
    let sigma_wy = shear_rebar_issue.is_none().then(|| {
        sepika_core::material_grade::shear_rebar_yield_strength(shear_rebar)
            .unwrap_or(opts.sigma_wy)
    });
    let opts_owned = UltimateShearOptions { rp, ..opts.clone() };
    let opts = &opts_owned;
    let main_rebar_strength = if main_rebar_issue.is_some() {
        None
    } else {
        Some(
            main_rebar
                .and_then(|material| {
                    sepika_core::material_grade::rebar_yield_strength(Some(material))
                        .filter(|fy| fy.is_finite())
                        .map(|fy| (material, fy))
                })
                .ok_or_else(|| format!("部材 ID {} の主筋の降伏強度を解決できません", elem.id.0))?,
        )
    };
    let sigma_y = main_rebar_strength.map(|(_, fy)| fy);
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
    let beam_support_reason = static_beam_rebar.and_then(|rebar| {
        static_beam_support_reason(elem, model, connection_counts).or_else(|| {
            if demand.static_beam_hinge_mechanism.is_none() {
                Some("ultimate_beam_hinge_mechanism_unverified: 想定する載荷方向で両端の強軸曲げ降伏を確認できません".into())
            } else if rebar.top != rebar.bottom {
                Some("ultimate_beam_hinge_bending_unverified: 上下の主筋配置が異なり、単一の引張側を両端に用いる略算の適用を確認できません".into())
            } else {
                None
            }
        })
    });
    let need_be = opts.shear_method == ShearMethod::Ductility;
    let beam_applicability = main_rebar_strength
        .filter(|_| kind == MemberKind::Girder)
        .map(|(material, fy)| beam_mu_applicability_reason(mat, fc, material, fy, demand));
    let mut static_beam_reasons = Vec::new();
    if static_beam_rebar.is_some() {
        static_beam_reasons.extend(beam_support_reason.iter().cloned());
        static_beam_reasons.extend(beam_applicability.iter().cloned());
        static_beam_reasons.push(BEAM_SHEAR_UPPER_BOUND_UNVERIFIED.into());
        if let Some(mechanism) = demand.static_beam_hinge_mechanism {
            let i_top = mechanism == StaticBeamHingeMechanism::ITopJBottom;
            for (end, top) in [("i端", i_top), ("j端", !i_top)] {
                let props = rc_bar_props(shape, RcDirection::Strong, top, need_be)
                    .ok_or_else(invalid_rebar)?;
                static_beam_reasons.push(beam_mu_direction_detail(&props, end));
            }
        }
    }
    if let (Some(rebar), Some(_)) = (static_beam_rebar, &beam_support_reason) {
        let reason = match demand.static_beam_bending {
            StaticBeamBending::Mixed => Some("ultimate_beam_mz_mixed_signs: 長期強軸曲げに正負が混在し、参考断面値の引張側を決定できません"),
            StaticBeamBending::Unverified => Some("ultimate_beam_mz_unverified: 長期強軸曲げの符号を確認できず、参考断面値の引張側を決定できません"),
            StaticBeamBending::Signed if demand.mz == 0.0 => Some("ultimate_beam_mz_zero: 長期強軸曲げが0で、参考断面値の引張側を決定できません"),
            StaticBeamBending::Signed => None,
        };
        if let Some(reason) = reason {
            if rebar.is_unset() || rebar.validate(b, d).is_err() {
                return Err(invalid_rebar());
            }
            let reason = match &l_clear {
                Ok(_) => {
                    static_beam_reasons.push(reason.into());
                    static_beam_reasons.join("。")
                }
                Err(reason) => reason.clone(),
            };
            return unchecked(&reason);
        }
    }

    let beam_mechanism = match (static_beam_rebar, &beam_support_reason) {
        (Some(_), None) => demand.static_beam_hinge_mechanism,
        _ => None,
    };
    let tension_is_top = beam_mechanism.map_or_else(
        || strong_tension_is_top(shape, demand.mz, need_be),
        |mechanism| mechanism == StaticBeamHingeMechanism::ITopJBottom,
    );
    let p = rc_bar_props(shape, RcDirection::Strong, tension_is_top, need_be)
        .ok_or_else(invalid_rebar)?;
    let l_clear = match l_clear {
        Ok(span) => span,
        Err(reason) => return unchecked(&reason),
    };
    if static_beam_rebar.is_some() && p.at <= 0.0 {
        let reason = if tension_is_top {
            "ultimate_beam_tension_rebar_missing: 上端引張の主筋がありません"
        } else {
            "ultimate_beam_tension_rebar_missing: 下端引張の主筋がありません"
        };
        static_beam_reasons.push(reason.into());
        return unchecked(&static_beam_reasons.join("。"));
    }
    let jt = 7.0 * p.d_eff / 8.0;
    let at = p.at;
    let ag = p.ag;
    let pw = p.pw;
    let n_axial = demand.n_axial;

    let mu = sigma_y.map_or(f64::NAN, |sigma_y| match kind {
        MemberKind::Column => column_mu(p.b_dir, p.d_dir, p.dt, at, ag, sigma_y, fc, n_axial),
        _ => {
            let reference = rc_mu_simple(&RcCapacityInput {
                b: p.b_dir,
                d: p.d_dir,
                at,
                d_eff: p.d_eff,
                sigma_y,
                fc,
                pw,
                sigma_wy: 0.0,
                clear_span: l_clear,
                sigma_0: 0.0,
            });
            if reference.is_finite() && reference > 0.0 {
                reference
            } else {
                f64::NAN
            }
        }
    });
    let beam_reference = beam_applicability.as_ref().map(|_| {
        let direction = beam_mu_direction_detail(&p, "");
        let value = if mu.is_finite() {
            format!("参考曲げ略算 Mu={mu:.0} N·mm")
        } else {
            "参考曲げ略算 Muは未算定".into()
        };
        let selection = if demand.mode == UltimateDemandMode::ConfirmedTotal && demand.mz == 0.0 {
            "応答Mz=0のため引張鉄筋量の小さい側を参照。"
        } else {
            ""
        };
        format!("{value}（{selection}{direction}）")
    });

    let shear_strength = member_shear_strength(&p, fc, n_axial, l_clear, sigma_wy, opts);
    let qsu = shear_strength.unwrap_or(f64::NAN);

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
                rp,
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
                rp,
                tensile_axial: n_axial < 0.0,
                yield_hinge: rp > 0.0,
                lightweight: opts.lightweight,
            }),
        };
        (qbu, tau_bu)
    };
    let bond_unknown_bar_type = opts.include_bond
        && main_rebar_issue.is_some()
        && !matches!(p.main_dia, RebarSize::Standard(_));
    let bond_uses_transverse_rebar = opts.shear_method == ShearMethod::Ductility
        || (p.shear_pitch > 0.0 && p.shear_legs > 0 && p.shear_dia.area_mm2() > 0.0);
    let bond_bar_type_unverified = bond_unknown_bar_type || round_bond_reason.is_some();
    let bond_zero_upper = opts.include_bond
        && !bond_bar_type_unverified
        && shear_category_issue.is_some()
        && opts.shear_method == ShearMethod::Ductility
        && ductility_truss_upper_bound(
            truss_lambda(p.shear_pitch, jt, p.be, p.n_s),
            ductility_nu(fc, rp) * fc,
            p.be,
            jt,
        ) == 0.0;
    let bond_unverified = bond_bar_type_unverified
        || (opts.include_bond
            && shear_category_issue.is_some()
            && bond_uses_transverse_rebar
            && !bond_zero_upper);
    let main_rebar_issue = main_rebar_issue.map(|reason| {
        if bond_unknown_bar_type {
            format!("{reason}。数値径から異形鉄筋であることを確認できないため付着は未算定です")
        } else {
            reason
        }
    });
    let (qbu, tau_bu) = match (opts.include_bond, bond_unverified) {
        (false, _) => (0.0, 0.0),
        // Vbu2 が0なら拘束効果を含む τbu が未算定でも Vbu は0となる。
        (true, false) if bond_zero_upper => (0.0, f64::NAN),
        (true, false) => bond_strength(&p),
        (true, true) => (f64::NAN, f64::NAN),
    };
    if static_beam_rebar.is_some() {
        static_beam_reasons.extend(beam_reference);
        static_beam_reasons.extend(main_rebar_issue);
        static_beam_reasons.extend(round_bond_reason);
        static_beam_reasons.extend(shear_rebar_issue.iter().cloned());
        let ql = demand.q_long.unwrap_or_default().abs();
        let shear_ng = beam_mechanism.is_some() && qsu.is_finite() && ql > qsu;
        let bond_ng = beam_mechanism.is_some()
            && opts.include_bond
            && qbu.is_finite()
            && ql > qbu
            && (bond_zero_upper || {
                // QLの最大位置と各端の付着耐力を対応付けられないため、両側での超過を要する。
                let (opposite_qbu, _) = bond_strength(&RcBarProps {
                    top_bar: !p.top_bar,
                    ..p
                });
                opposite_qbu.is_finite() && ql > opposite_qbu
            });
        let gravity_ng = shear_ng || bond_ng;
        if shear_ng {
            static_beam_reasons.push(format!("長期せん断力 {ql:.0} N がせん断耐力を超えます"));
        }
        if bond_ng {
            static_beam_reasons.push(format!(
                "長期せん断力 {ql:.0} N が上端引張・下端引張の双方の付着耐力を超えます"
            ));
        }
        let reason = static_beam_reasons.join("。");
        if sigma_y.is_none() && !gravity_ng {
            return Ok(Some(unchecked_without_bond_values(&reason)));
        }
        let qsu_detail = if shear_strength.is_none() {
            "未算定".into()
        } else {
            format!("{qsu:.0} N")
        };
        let bond_margin = if bond_ng { 0.0 } else { f64::NAN };
        let bond_detail = bond_detail("付着余裕度", qbu, tau_bu, bond_margin);
        return Ok(Some(UltimateCheck {
            mu,
            qsu,
            qbu,
            shear_margin: if shear_ng { 0.0 } else { f64::NAN },
            bond_margin,
            unchecked_reason: (!gravity_ng).then(|| reason.clone()),
            detail: format!("Qmu=未算定, {shear_symbol}={qsu_detail}, {bond_detail}。{reason}"),
            ..unchecked_member(elem.id, kind, &reason)
        }));
    }

    let qmu = match demand.shear {
        Some(qm) => Ok(opts.upper_strength_factor * qm.abs()),
        None if sigma_y.is_some() => {
            static_hinge_shear(mu, opts.upper_strength_factor, l_clear, "強軸")
        }
        None => Ok(f64::NAN),
    };
    let strong_hinge_issue = qmu.as_ref().err().map(String::as_str);
    let qmu = qmu.as_ref().copied().unwrap_or(f64::NAN);
    let biaxial_shear_requested = kind == MemberKind::Column && opts.biaxial_shear;
    let static_biaxial_shear =
        biaxial_shear_requested && demand.mode == UltimateDemandMode::StaticHinge;
    let ql = demand.q_long.map(|q| q.abs()).unwrap_or(0.0);
    let gravity_exceeds_capacity = |capacity_n: f64| ql > capacity_n;
    let margin = |capacity_n: f64| {
        if demand.mode == UltimateDemandMode::StaticHinge && gravity_exceeds_capacity(capacity_n) {
            0.0
        } else if qmu.is_nan() {
            f64::NAN
        } else if qmu > 0.0 {
            (capacity_n - ql).max(0.0) / qmu
        } else {
            f64::INFINITY
        }
    };
    let shear_margin = shear_strength.map_or(f64::NAN, margin);
    let bond_margin = match (opts.include_bond, bond_unverified) {
        (false, _) => f64::INFINITY,
        (true, false) => margin(qbu),
        (true, true) => f64::NAN,
    };

    let mut weak_shear_ng = false;
    let mut weak_hinge_issue = None;
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
        let shear_strength_y = member_shear_strength(&p_y, fc, n_axial, l_clear, sigma_wy, opts);
        if !static_biaxial_shear && shear_strength_y.is_none() {
            return Ok(None);
        }
        let qmu_y = match demand.shear_weak {
            Some(qmy) => opts.upper_strength_factor * qmy.abs(),
            None => {
                let fy = sigma_y.ok_or("二軸せん断に必要な主筋の降伏強度を解決できません")?;
                let mu_y = column_mu(
                    p_y.b_dir, p_y.d_dir, p_y.dt, p_y.at, p_y.ag, fy, fc, n_axial,
                );
                match static_hinge_shear(mu_y, opts.upper_strength_factor, l_clear, "弱軸") {
                    Ok(shear_n) => shear_n,
                    Err(reason) => {
                        weak_hinge_issue = Some(reason);
                        return Ok(None);
                    }
                }
            }
        };
        let Some(qsu_y) = shear_strength_y else {
            return Ok(None);
        };
        let Some(qsu) = shear_strength.filter(|_| !qmu.is_nan()) else {
            weak_shear_ng = qmu_y > qsu_y;
            return Ok(None);
        };
        let zero_capacity_with_unknown_rebar = |demand_n, capacity_n| {
            shear_rebar_issue.is_some() && capacity_n == 0.0 && demand_n == 0.0
        };
        if static_biaxial_shear {
            return Ok(Some(static_biaxial_margin(
                (qmu, qsu),
                (qmu_y, qsu_y),
                shear_rebar_issue.is_some(),
            )));
        }
        let ratio = |demand_n: f64, capacity_n: f64| {
            if capacity_n > 0.0 {
                demand_n / capacity_n
            } else if zero_capacity_with_unknown_rebar(demand_n, capacity_n) {
                0.0
            } else {
                f64::INFINITY
            }
        };
        Ok(Some(biaxial_margin(
            ratio(qmu, qsu),
            ratio(qmu_y, qsu_y),
            2.0,
        )))
    });
    let biaxial_shear_margin = biaxial_shear.and_then(Result::ok).flatten();

    let biaxial_bending_margin = if let Some(sigma_y) =
        sigma_y.filter(|_| kind == MemberKind::Column && opts.biaxial_bending)
    {
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

    let axial = sigma_y
        .filter(|_| kind == MemberKind::Column)
        .map(|fy| rc_column_axial_ultimate(p.b_dir, p.d_dir, fc, ag, fy));

    let demand_symbol = demand.mode.strong_shear_demand_symbol();
    let demand_detail = match demand.mode {
        UltimateDemandMode::StaticHinge if qmu.is_nan() => {
            format!("Qmu=未算定, QL={ql:.0} N（強軸）")
        }
        UltimateDemandMode::StaticHinge if qmu == 0.0 => {
            format!("Qmu=0 N, QL={ql:.0} N（強軸の長期せん断力を耐力と直接比較）")
        }
        UltimateDemandMode::StaticHinge if qmu <= 0.5 => {
            format!("{demand_symbol}={qmu:e} N, QL={ql:.0} N（強軸）")
        }
        UltimateDemandMode::StaticHinge => {
            format!("{demand_symbol}={qmu:.0} N, QL={ql:.0} N（強軸）")
        }
        UltimateDemandMode::ConfirmedTotal => {
            format!("{demand_symbol}={qmu:.0} N（初期重力込み）")
        }
    };
    let margin_label = |label: &str, capacity| {
        if qmu > 0.0 {
            format!(
                "{label} {}",
                demand.mode.strong_shear_margin_formula(capacity)
            )
        } else {
            label.to_owned()
        }
    };
    let shear_margin_label = margin_label("強軸せん断余裕度", shear_symbol);
    let bond_margin_label = margin_label("付着余裕度", bond_symbol);
    let format_margin = |margin: f64| {
        if strong_hinge_issue.is_some() && margin.is_nan() {
            "未算定".into()
        } else {
            format!("{margin:.3}")
        }
    };
    let shear_detail = if shear_strength.is_none() {
        format!("{shear_symbol}=未算定, {shear_margin_label}=未算定")
    } else {
        format!(
            "{shear_symbol}={qsu:.0} N, {shear_margin_label}={}",
            format_margin(shear_margin)
        )
    };
    if let Some(reason) = main_rebar_issue {
        let reason =
            combine_incomplete_reasons(Some(reason), shear_rebar_issue.as_deref()).unwrap();
        let shear_ng = shear_margin < 1.0;
        let biaxial_shear_ng = biaxial_shear_margin.is_some_and(|margin| margin < 1.0);
        let bond_ng = bond_margin < 1.0;
        let mut check = unchecked_member(elem.id, kind, &reason);
        if shear_ng || biaxial_shear_ng || weak_shear_ng || bond_ng {
            check.unchecked_reason = None;
            check.qmu = qmu;
            check.detail.push_str(&format!("。{demand_detail}"));
            if shear_ng || biaxial_shear_ng {
                check.qsu = qsu;
                check.shear_margin = shear_margin;
                check.biaxial_shear_margin = biaxial_shear_margin.filter(|margin| *margin < 1.0);
                check.detail.push_str(&format!("。{shear_detail}"));
                if shear_ng {
                    check.detail.push_str("。強軸せん断耐力を超えます");
                }
                if let Some(margin) = check.biaxial_shear_margin {
                    check.detail.push_str(&format!(
                        "。二軸せん断の組合せで耐力を超えます（二軸せん断余裕度={margin:.3}）"
                    ));
                }
            }
            if weak_shear_ng {
                check.detail.push_str("。弱軸せん断耐力を超えます");
            }
            if bond_ng {
                check.qbu = qbu;
                check.bond_margin = bond_margin;
                check.detail.push_str("。付着耐力を超えます");
            }
        }
        let bond_detail = if bond_ng {
            bond_detail(&bond_margin_label, qbu, tau_bu, bond_margin)
        } else {
            unavailable_bond_detail()
        };
        check.detail.push_str(&format!("。{bond_detail}"));
        return Ok(Some(check));
    }

    let comparison_ok =
        |margin: f64| margin >= 1.0 || (strong_hinge_issue.is_some() && margin.is_nan());
    let shear_ok = (shear_strength.is_none() || comparison_ok(shear_margin))
        && biaxial_shear_margin.is_none_or(|m| m >= 1.0)
        && !weak_shear_ng;
    let bending_ok = biaxial_bending_margin.map(|m| m >= 1.0).unwrap_or(true);
    let checked_parts_ok =
        shear_ok && (bond_unverified || comparison_ok(bond_margin)) && bending_ok;
    let hinge_issue = combine_incomplete_reasons(
        strong_hinge_issue.map(str::to_owned),
        weak_hinge_issue.as_deref(),
    );
    let unchecked_reason = if checked_parts_ok {
        let reason = combine_incomplete_reasons(
            biaxial_shear.and_then(Result::err).map(str::to_owned),
            round_bond_reason.as_deref(),
        );
        let reason = combine_incomplete_reasons(reason, shear_rebar_issue.as_deref());
        combine_incomplete_reasons(reason, hinge_issue.as_deref())
    } else {
        None
    };
    let ok = checked_parts_ok && unchecked_reason.is_none();

    let shear_label = match opts.shear_method {
        ShearMethod::Plastic => "塑性理論式",
        ShearMethod::Ductility => "靭性指針式",
    };
    let basis = match kind {
        MemberKind::Column => {
            format!("RC柱 終局検定（{shear_label} {shear_symbol}/{bond_symbol}）")
        }
        _ => format!("RC梁 終局検定（{shear_label} {shear_symbol}/{bond_symbol}）"),
    };
    let biaxial_str = match biaxial_shear_margin {
        Some(m) => format!(", 二軸せん断余裕度={m:.3}"),
        None => String::new(),
    };
    let ql_str = if static_biaxial_shear {
        let weak = demand
            .q_long_weak
            .map_or_else(|| "未取得".into(), |q| q.abs().to_string());
        format!(", 弱軸の QL[N]={weak}")
    } else {
        String::new()
    };
    let bend_str = match biaxial_bending_margin {
        Some(m) => format!(", 2軸曲げ余裕度={m:.3}"),
        None => String::new(),
    };
    let bending_detail = match (beam_reference, beam_applicability) {
        (Some(reference), Some(reason)) => format!("{reference}。{reason}"),
        _ => format!("Mu={mu:.0} N·mm"),
    };
    let bond_detail = bond_detail(&bond_margin_label, qbu, tau_bu, bond_margin);
    let mut detail = format!(
        "{bending_detail}, {demand_detail}, {shear_detail}, {bond_detail}\
         {biaxial_str}{bend_str}{ql_str}, pw={pw:.5}, jt={jt:.1} mm, L={l_clear} mm, Rp={rp:.4}",
    );
    if weak_shear_ng {
        detail.push_str("。弱軸せん断耐力を超えます");
    }
    for reason in [&round_bond_reason, &shear_rebar_issue, &hinge_issue]
        .into_iter()
        .flatten()
    {
        detail.push('。');
        detail.push_str(reason);
    }

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
/// 鉄筋の材料区分や内法長さ等の適用条件を満たさない部材は、未検定理由を保持する。
pub fn collect_rc_ultimate_checks(
    model: &Model,
    demand_by_elem: &[(ElemId, MemberDemand)],
    opts: &UltimateShearOptions,
) -> Result<Vec<UltimateCheck>, String> {
    let connection_counts = node_connection_counts(model);
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
        if let Some(mut check) =
            check_member(elem, sec, mat, model, &connection_counts, demand, opts)?
        {
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
