//! RC梁・矩形柱・円形柱の終局検定ドライバ。
//!
//! - [`UltimateCheck`] — 1 部材分の終局検定結果。
//! - [`collect_rc_ultimate_checks`] — モデルの RC梁・矩形柱・円形柱を一括検定する。

use std::collections::HashMap;

use num_rational::BigRational;
use num_traits::ToPrimitive;

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
    MemberDemand, StaticBeamBending, StaticBeamHingeMechanism, UltimateDemandMode,
    UltimateShearOptions,
};
use super::rc_axial::{rc_column_axial_ultimate, RcAxialUltimate};
use super::rc_bond::{
    RcUltimateBondAxis, RcUltimateBondDemand, RcUltimateBondInput, RcUltimateBondStress,
};
use super::rc_props::{rc_bar_props, RcBarProps, RcDirection};
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
    /// 終局せん断強度 Qsu [N]。算定条件を確認できない場合は、他項目の不適合にかかわらず NaN。
    pub qsu: f64,
    /// 確認できた組のうち付着余裕度が最小の応力度と許容値 [N/mm²]。他の組の未算定時も保持する。
    /// 無効、併用条件の不整合、または算定できる組がない場合は None。
    pub bond_stress: Option<super::rc_bond::RcUltimateBondStress>,
    /// 応力度とは独立したカットオフ筋の必要延長の適否。対象外または適否未確認は None。
    /// 併用条件が不整合でも、個別に確認できた延長不足は Some(false)。
    pub bond_development_length_ok: Option<bool>,
    /// 確定総応答の強軸せん断余裕度。耐力が算定済みで需要が 0 の場合は無限大。
    /// 未算定値は、他項目の不適合にかかわらず NaN。
    pub shear_margin: f64,
    /// 確定総応答の二軸せん断余裕度。`biaxial_shear` が有効で両軸の耐力を算定できる場合に返す。
    pub biaxial_shear_margin: Option<f64>,
    /// 柱の二軸曲げ余裕度。`biaxial_bending` が有効で算定条件を確認できる場合に返す。
    /// `1/((Mmx/Mux)^2+(Mmy/Muy)^2)^(1/2)`。設計用曲げ需要が 0 なら `f64::INFINITY`。
    pub biaxial_bending_margin: Option<f64>,
    /// 付着余裕度 Kfb/τD。算定済み需要が 0 の場合、または通常の検定結果で付着が無効なら無限大。
    /// 未算定は NaN。部材の検定を開始できない場合は、付着が無効でも NaN。
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
fn strong_tension_is_top(shape: &SectionShape, mz: f64) -> bool {
    if mz < 0.0 {
        return true;
    }
    if mz > 0.0 {
        return false;
    }
    let at_top = rc_bar_props(shape, RcDirection::Strong, true).map(|p| p.at);
    let at_bottom = rc_bar_props(shape, RcDirection::Strong, false).map(|p| p.at);
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

fn unchecked_member(elem: ElemId, kind: MemberKind, reason: &str) -> UltimateCheck {
    UltimateCheck {
        elem,
        kind,
        mu: f64::NAN,
        qmu: f64::NAN,
        qsu: f64::NAN,
        bond_stress: None,
        bond_development_length_ok: None,
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

/// 非対象断面は `Ok(None)`。検定結果を確定できない断面・材料の不備は部材 ID 付きで `Err`。
/// せん断補強筋の不備はせん断を未算定とし、確認済みの独立した照査結果を保持する。
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
    let shear_symbol = "Qsu";
    let bond_detail = |stress: Option<super::rc_bond::RcUltimateBondStress>, margin: f64| {
        if !opts.include_bond {
            "τD=無効, Kfb=無効, 付着余裕度=無効".into()
        } else if let Some(stress) = stress {
            format!(
                "τD={:.3} N/mm², Kfb={:.3} N/mm², 付着余裕度={margin:.3}",
                stress.demand_mpa, stress.capacity_mpa
            )
        } else {
            "τD=未算定, Kfb=未算定, 付着余裕度=未算定".into()
        }
    };
    let unavailable_bond_detail = || bond_detail(None, f64::NAN);
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
    let shear_rebar_issue = shear_category_issue.or_else(|| {
        shear_rebar
            .and_then(|material| {
                sepika_core::material_grade::shear_rebar_fy_issue(material).or_else(|| {
                    sepika_core::material_grade::shear_rebar_material_issue(Some(material))
                })
            })
            .or_else(|| shape.validate_rebar_size_materials(None, shear_rebar).err())
            .map(|reason| format!("部材 ID {}（断面「{}」）の{reason}", elem.id.0, sec.name))
    });
    let shear_pitch_issue = match shape {
        SectionShape::RcBeamRect { rebar, .. }
            if rebar.aw_mm2() > 0.0 && !rebar.stirrup.pitch.is_finite() =>
        {
            Some("ultimate_beam_stirrup_pitch_nonfinite: あばら筋間隔が非有限です。断面のあばら筋間隔を確認してください".to_owned())
        }
        _ => None,
    };
    let grade = main_rebar_grade(main_rebar);
    let round_bond_reason = (opts.include_bond
        && main_rebar_issue.is_none()
        && matches!(grade, "SR235" | "SR295"))
    .then(|| {
        format!(
            "ultimate_bond_round_bar_unsupported: 部材 ID {} の主筋は丸鋼（{grade}）です。大地震時の異形鉄筋の付着照査を適用できないため、付着は未算定です",
            elem.id.0
        )
    });
    let member_incompleteness = |reason: &str| {
        let reason = main_rebar_issue
            .as_ref()
            .map_or_else(|| reason.to_owned(), |issue| format!("{issue}。{reason}"));
        [&shear_pitch_issue, &shear_rebar_issue, &round_bond_reason]
            .into_iter()
            .flatten()
            .fold(reason, |reason, issue| format!("{reason}。{issue}"))
    };
    let unchecked = |reason: &str| {
        let reason = member_incompleteness(reason);
        Ok(Some(unchecked_without_bond_values(&reason)))
    };
    if [b, d].into_iter().any(|v| !v.is_finite() || v <= 0.0) {
        return Err(format!(
            "部材 ID {} の Fc または断面寸法が不正です",
            elem.id.0
        ));
    }
    let fc = mat.fc.unwrap_or(f64::NAN);
    let concrete_type_issue = (mat.category != sepika_core::model::MaterialCategory::Concrete)
        .then(|| {
            format!(
                "部材 ID {} の断面主材料に「コンクリート」区分を割り当ててください",
                elem.id.0
            )
        });
    let concrete_error = match mat.fc {
        None => Some(format!(
            "部材 ID {} のコンクリート強度 Fc が未設定です",
            elem.id.0
        )),
        Some(_) if !fc.is_finite() || fc <= 0.0 => Some(format!(
            "部材 ID {} の Fc または断面寸法が不正です",
            elem.id.0
        )),
        Some(_) => None,
    };
    if let Err(reason) =
        shape.validate_rebar_size_materials(main_rebar.filter(|_| main_rebar_issue.is_none()), None)
    {
        if main_rebar_issue.is_some() || shear_rebar_issue.is_some() {
            return unchecked(&reason);
        }
        return Err(format!("部材 ID {} の{reason}", elem.id.0));
    }
    let main_rebar_strength =
        main_rebar
            .filter(|_| main_rebar_issue.is_none())
            .and_then(|material| {
                sepika_core::material_grade::rebar_yield_strength(Some(material))
                    .filter(|fy| fy.is_finite())
                    .map(|fy| (material, fy))
            });
    let main_rebar_strength_error = (main_rebar_issue.is_none() && main_rebar_strength.is_none())
        .then(|| format!("部材 ID {} の主筋の降伏強度を解決できません", elem.id.0));
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
    let check_bond_input = |input: RcUltimateBondInput| {
        let l_clear = *l_clear
            .as_ref()
            .map_err(|_| "付着照査の内法長さを確認できません")?;
        let one_layer = match shape {
            SectionShape::RcBeamRect { rebar, .. } => {
                if matches!(input.axis, RcUltimateBondAxis::Weak) {
                    return Err("RC梁の付着照査は強軸方向に限ります");
                }
                rebar.top.len() == 1 && rebar.bottom.len() == 1
            }
            SectionShape::RcColumnRect { rebar, .. } => rebar.x.len() == 1 && rebar.y.len() == 1,
            _ => return Err("終局付着は矩形断面のRC梁・柱に限ります"),
        };
        if !one_layer {
            return Err("終局付着は一段配筋に限ります");
        }
        let direction = match input.axis {
            RcUltimateBondAxis::Strong => RcDirection::Strong,
            RcUltimateBondAxis::Weak => RcDirection::Weak,
        };
        let props = rc_bar_props(shape, direction, true)
            .ok_or("付着照査の検討方向の配筋を確認できません")?;
        let other = rc_bar_props(shape, direction, false)
            .ok_or("付着照査の検討方向の配筋を確認できません")?;
        if ![props.d_eff, other.d_eff]
            .into_iter()
            .all(|depth| (input.effective_depth_mm - depth).abs() <= depth * 1e-9)
        {
            return Err("付着照査の有効せいが検討方向の配筋と一致しません");
        }
        if (input.clear_span_mm() - l_clear).abs() > l_clear * 1e-9 {
            return Err("付着照査の内法長さが部材の内法長さと一致しません");
        }
        if input.bar_designation_mm != props.main_dia.designation_or_diameter_mm() {
            return Err("付着照査の鉄筋呼び名がモデルと一致しません");
        }
        let mut check = input.check(fc, mat.concrete_class)?;
        if concrete_type_issue.is_some() {
            check.stress = Err("終局付着照査に必要な主材料のコンクリート区分を確認できません");
        } else if sigma_y.is_none() {
            check.stress = Err("終局付着照査に必要な主筋の降伏強度を確認できません");
        } else if !matches!(props.main_dia, RebarSize::Standard(_)) {
            check.stress = Err("終局付着照査には主筋の標準呼び名（D41以下）が必要です");
        } else if main_rebar.is_none_or(|material| {
            sepika_core::material_grade::rc_standard_deformed_rebar_fy(material, props.main_dia)
                .is_none()
        }) {
            check.stress =
                Err("終局付着照査には、材種と基準強度が一致するD41以下のJIS異形鉄筋が必要です");
        } else if input
            .design_yield_stress_mpa
            .is_some_and(|design| sigma_y.is_some_and(|nominal| design < nominal))
        {
            check.stress = Err("付着照査の設計用降伏応力度が主筋の規格降伏強度を下回っています");
        }
        Ok(check)
    };
    let stress_margin = |stress: RcUltimateBondStress| stress.capacity_mpa / stress.demand_mpa;
    let (mut bond_stress, mut bond_issue, mut bond_development_length_ok) = (None, None, None);
    if opts.include_bond {
        if let Some(reason) = &round_bond_reason {
            bond_issue = Some(reason.as_str());
        } else if let Some(bond) = demand.bond {
            let (inputs, grouping_issue) = bond.inputs();
            let inconsistent_pair = grouping_issue.is_some()
                && matches!(bond, RcUltimateBondDemand::ThroughAndCutoff { .. });
            bond_issue = grouping_issue;
            for input in inputs.into_iter().flatten() {
                match check_bond_input(input) {
                    Ok(check) => {
                        if let Some(length_ok) = check
                            .development_length_ok
                            .filter(|&length_ok| !inconsistent_pair || !length_ok)
                        {
                            bond_development_length_ok =
                                Some(bond_development_length_ok.unwrap_or(true) && length_ok);
                        }
                        if inconsistent_pair {
                            continue;
                        }
                        match check.stress {
                            Ok(stress) => {
                                if bond_stress.is_none_or(|governing_stress| {
                                    stress_margin(stress) < stress_margin(governing_stress)
                                }) {
                                    bond_stress = Some(stress);
                                }
                            }
                            Err(reason) => {
                                bond_issue.get_or_insert(reason);
                            }
                        }
                    }
                    Err(reason) => {
                        bond_issue.get_or_insert(reason);
                    }
                }
            }
        } else {
            bond_issue = Some("終局付着を未算定とします。検討軸、通し筋・カットオフ筋の区分、必要な付着長さと補強条件を確認できません");
        }
    };
    let bond_margin = if opts.include_bond {
        bond_stress.map_or(f64::NAN, stress_margin)
    } else {
        f64::INFINITY
    };
    let bond_length_issue = (bond_development_length_ok == Some(false))
        .then_some("カットオフ筋の付着長さが必要な延長を満たしません");
    let bond_ng = bond_margin < 1.0 || bond_length_issue.is_some();
    let set_bond_result = |check: &mut UltimateCheck| {
        check.bond_stress = bond_stress;
        check.bond_development_length_ok = bond_development_length_ok;
        check.bond_margin = bond_margin;
        check
            .detail
            .push_str(&format!("。{}", bond_detail(bond_stress, bond_margin)));
        for issue in [bond_issue, bond_length_issue].into_iter().flatten() {
            check.detail.push('。');
            check.detail.push_str(issue);
        }
        if bond_development_length_ok == Some(true) {
            check
                .detail
                .push_str("。カットオフ筋の必要延長を満たします");
        }
    };
    let unchecked_bending = |reason: &str| {
        let reason = member_incompleteness(reason);
        let mut check = unchecked_member(elem.id, kind, &reason);
        check.unchecked_reason = if bond_ng {
            None
        } else {
            combine_incomplete_reasons(Some(reason), bond_issue)
        };
        set_bond_result(&mut check);
        Ok(Some(check))
    };
    if let Some(reason) = concrete_type_issue {
        return unchecked_bending(&reason);
    }
    let mut material_errors = [concrete_error, main_rebar_strength_error]
        .into_iter()
        .flatten();
    if let Some(mut reason) = material_errors.next() {
        if bond_stress.is_none() && bond_development_length_ok.is_none() {
            return Err(reason);
        }
        if let Some(other) = material_errors.next() {
            reason.push('。');
            reason.push_str(&other);
        }
        return unchecked_bending(&reason);
    }
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
                let props =
                    rc_bar_props(shape, RcDirection::Strong, top).ok_or_else(invalid_rebar)?;
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
            return unchecked_bending(&reason);
        }
    }

    let beam_mechanism = match (static_beam_rebar, &beam_support_reason) {
        (Some(_), None) => demand.static_beam_hinge_mechanism,
        _ => None,
    };
    let tension_is_top = beam_mechanism.map_or_else(
        || strong_tension_is_top(shape, demand.mz),
        |mechanism| mechanism == StaticBeamHingeMechanism::ITopJBottom,
    );
    let p = rc_bar_props(shape, RcDirection::Strong, tension_is_top).ok_or_else(invalid_rebar)?;
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
        return unchecked_bending(&static_beam_reasons.join("。"));
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

    let shear_applicability = if demand.mode == UltimateDemandMode::StaticHinge {
        Err("せん断スパン比の算定には確定総応答のMとQが必要です")
    } else if demand.min_axial_for_shear_n.is_none() {
        Err("部材荷重があるか、フェース位置の断面力の範囲を確認できません")
    } else if matches!(shape, SectionShape::RcColumnCircle { .. }) {
        Err("荒川mean式の終局検定は矩形断面に限ります")
    } else if kind == MemberKind::Girder && crate::beam_has_attached_slab(model, elem) {
        Err("スラブ付きRC梁の終局せん断の適用条件を確認できません")
    } else if mat.concrete_class != sepika_core::units::ConcreteClass::Normal
        || !(18.0..=60.0).contains(&fc)
    {
        Err("荒川mean式には普通コンクリートでFcが18〜60 N/mm²の範囲を用います")
    } else if !main_rebar.is_some_and(sepika_core::material_grade::has_jis_rebar_strength_limit) {
        Err("荒川mean式の適用に必要なJIS主筋を確認できません（大臣認定材料は対象外）")
    } else if shear_pitch_issue.is_some() {
        Err("せん断補強筋間隔を確認できません")
    } else {
        demand
            .min_axial_for_shear_n
            .filter(|&axial| axial.is_finite() && (0.0..=n_axial).contains(&axial))
            .filter(|&axial| kind == MemberKind::Column || (axial == 0.0 && n_axial == 0.0))
            .ok_or("終局せん断に必要な軸力の範囲（梁は軸力なし、柱は非引張）を確認できません")
    };
    let confirmed_shear_rebar = shear_rebar.filter(|_| shear_rebar_issue.is_none());
    let shear_result = shear_applicability.and_then(|axial| {
        let capacity = |props| {
            member_shear_strength(
                props,
                fc,
                axial,
                l_clear,
                demand.mz,
                demand.shear,
                confirmed_shear_rebar,
            )
        };
        let qsu = capacity(&p)?;
        // 確定総応答の曲げは絶対値のみ保持するため、片側の配筋だけでは耐力を確定できない。
        let other = rc_bar_props(shape, RcDirection::Strong, !tension_is_top)
            .ok_or("反対側の配筋を確認できません")?;
        Ok(qsu.min(capacity(&other)?))
    });
    let shear_issue = shear_result.as_ref().err().copied();
    let shear_strength = shear_result.ok();
    let qsu = shear_strength.unwrap_or(f64::NAN);

    if static_beam_rebar.is_some() {
        static_beam_reasons.extend(beam_reference);
        static_beam_reasons.extend(main_rebar_issue);
        static_beam_reasons.extend(bond_issue.map(str::to_owned));
        static_beam_reasons.extend(bond_length_issue.map(str::to_owned));
        static_beam_reasons.extend(shear_issue.map(str::to_owned));
        static_beam_reasons.extend(shear_rebar_issue.iter().cloned());
        static_beam_reasons.extend(shear_pitch_issue.iter().cloned());
        if bond_ng {
            static_beam_reasons.push("大地震時の付着の照査条件を満たしません".into());
        }
        let reason = static_beam_reasons.join("。");
        let bond_detail = bond_detail(bond_stress, bond_margin);
        return Ok(Some(UltimateCheck {
            mu,
            qsu,
            bond_stress,
            bond_development_length_ok,
            bond_margin,
            unchecked_reason: (!bond_ng).then(|| reason.clone()),
            detail: format!("Qmu=未算定, {shear_symbol}=未算定, {bond_detail}。{reason}"),
            ..unchecked_member(elem.id, kind, &reason)
        }));
    }

    let qmu = match demand.shear {
        Some(qm) => Ok(qm.abs()),
        None if sigma_y.is_some() => {
            static_hinge_shear(mu, opts.upper_strength_factor, l_clear, "強軸")
        }
        None => Ok(f64::NAN),
    };
    let strong_hinge_issue = qmu.as_ref().err().map(String::as_str);
    let qmu = qmu.as_ref().copied().unwrap_or(f64::NAN);
    let ql = demand.q_long.map_or(0.0, f64::abs);
    let shear_margin = shear_strength.map_or(f64::NAN, |capacity_n| capacity_n / qmu);
    let mut weak_shear_ng = false;
    let biaxial_shear = (kind == MemberKind::Column
        && opts.biaxial_shear
        && demand.mode == UltimateDemandMode::ConfirmedTotal)
        .then(|| {
            let p_y = rc_bar_props(shape, RcDirection::Weak, tension_is_top)
                .ok_or("二軸せん断に必要な弱軸の配筋形状または有効せいが不正です")?;
            let qsu_y = member_shear_strength(
                &p_y,
                fc,
                shear_applicability?,
                l_clear,
                demand.my,
                demand.shear_weak,
                confirmed_shear_rebar,
            )?;
            let qy = demand
                .shear_weak
                .ok_or("弱軸の総せん断力を確認できません")?
                .abs();
            weak_shear_ng = qy > qsu_y;
            Ok(shear_strength.map(|qsu| biaxial_margin(qmu / qsu, qy / qsu_y, 2.0)))
        });
    let biaxial_shear_margin = biaxial_shear.and_then(Result::ok).flatten();
    let weak_shear_issue = biaxial_shear
        .and_then(Result::err)
        .map(|reason: &str| format!("弱軸せん断は未算定（{reason}）"));

    let biaxial_bending_margin = if let Some(sigma_y) =
        sigma_y.filter(|_| kind == MemberKind::Column && opts.biaxial_bending)
    {
        rc_bar_props(shape, RcDirection::Weak, tension_is_top).map(|p_y| {
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
            format!("Qmu=0 N, QL={ql:.0} N（強軸）")
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
        if qmu > 0.0 && shear_strength.is_some() {
            format!(
                "{label} {}",
                demand.mode.strong_shear_margin_formula(capacity)
            )
        } else {
            label.to_owned()
        }
    };
    let shear_margin_label = margin_label("強軸せん断余裕度", shear_symbol);
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
        let reason =
            combine_incomplete_reasons(Some(reason), shear_pitch_issue.as_deref()).unwrap();
        let reason = combine_incomplete_reasons(Some(reason), shear_issue).unwrap();
        let reason = combine_incomplete_reasons(Some(reason), weak_shear_issue.as_deref()).unwrap();
        let shear_ng = shear_margin < 1.0;
        let biaxial_shear_ng = biaxial_shear_margin.is_some_and(|margin| margin < 1.0);
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
                check
                    .detail
                    .push_str("。大地震時の付着の照査条件を満たしません");
            }
        }
        if check.unchecked_reason.is_some() {
            check.unchecked_reason = combine_incomplete_reasons(check.unchecked_reason, bond_issue);
        }
        set_bond_result(&mut check);
        return Ok(Some(check));
    }

    let shear_ok = (shear_strength.is_none() || shear_margin >= 1.0)
        && biaxial_shear_margin.is_none_or(|m| m >= 1.0)
        && !weak_shear_ng;
    let bending_ok = biaxial_bending_margin.is_none_or(|m| m >= 1.0);
    let checked_parts_ok = shear_ok && !bond_ng && bending_ok;
    let unchecked_reason = if checked_parts_ok {
        let reason = combine_incomplete_reasons(weak_shear_issue.clone(), bond_issue);
        let reason = combine_incomplete_reasons(reason, shear_rebar_issue.as_deref());
        let reason = combine_incomplete_reasons(reason, shear_pitch_issue.as_deref());
        let reason = combine_incomplete_reasons(reason, shear_issue);
        combine_incomplete_reasons(reason, strong_hinge_issue)
    } else {
        None
    };
    let ok = checked_parts_ok && unchecked_reason.is_none();

    let basis = match kind {
        MemberKind::Column => {
            format!("RC柱 終局検定（荒川mean式 {shear_symbol}）")
        }
        _ => format!("RC梁 終局検定（荒川mean式 {shear_symbol}）"),
    };
    let biaxial_str = match biaxial_shear_margin {
        Some(m) => format!(", 二軸せん断余裕度={m:.3}"),
        None => String::new(),
    };
    let bend_str = match biaxial_bending_margin {
        Some(m) => format!(", 2軸曲げ余裕度={m:.3}"),
        None => String::new(),
    };
    let bending_detail = match (beam_reference, beam_applicability) {
        (Some(reference), Some(reason)) => format!("{reference}。{reason}"),
        _ => format!("Mu={mu:.0} N·mm"),
    };
    let bond_detail = bond_detail(bond_stress, bond_margin);
    let pw_detail = if shear_pitch_issue.is_some() {
        "未算定".into()
    } else {
        format!("{pw:.5}")
    };
    let mut detail = format!(
        "{bending_detail}, {demand_detail}, {shear_detail}, {bond_detail}\
         {biaxial_str}{bend_str}, pw={pw_detail}, jt={jt:.1} mm, L={l_clear} mm",
    );
    if weak_shear_ng {
        detail.push_str("。弱軸せん断耐力を超えます");
    }
    for reason in [
        bond_issue,
        bond_length_issue,
        shear_issue,
        weak_shear_issue.as_deref(),
        shear_rebar_issue.as_deref(),
        shear_pitch_issue.as_deref(),
        strong_hinge_issue,
    ]
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
        bond_stress,
        bond_development_length_ok,
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
