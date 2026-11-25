//! 地震時短期の設計用せん断力 QD = min(QD1, QD2)。

use crate::{DesignCtx, QdMethod, SeismicQd};

pub(crate) fn shear_demand_without_main_rebar(
    ctx: &DesignCtx,
    pos: f64,
    q_signed: f64,
    q_index: usize,
) -> Option<f64> {
    let Some(qd) = &ctx.seismic_qd else {
        return q_signed.is_finite().then_some(q_signed.abs());
    };
    if qd.method != QdMethod::Qd2 {
        return None;
    }
    let (_, forces) = qd.long_at.iter().find(|(p, _)| (p - pos).abs() < 1e-6)?;
    let demand = qd2(forces[q_index], q_signed, qd.n_factor);
    demand.is_finite().then_some(demand)
}

pub(crate) fn gravity_component(qd: &SeismicQd, pos: f64, index: usize) -> Option<f64> {
    if !pos.is_finite() {
        return None;
    }
    qd.long_at
        .iter()
        .find(|(p, _)| *p == pos)
        .map(|(_, forces)| forces[index])
        .filter(|value| value.is_finite())
}

pub(crate) fn gravity_shear_reason(index: usize) -> &'static str {
    if index == 1 {
        "Qy方向の設計用せん断力に必要な同位置の重力参照内力が不足または非有限です"
    } else {
        "Qz方向の設計用せん断力に必要な同位置の重力参照内力が不足または非有限です"
    }
}

pub(crate) fn column_qd2_reference_issue(
    qd: &SeismicQd,
    pos: f64,
    index: usize,
) -> Option<&'static str> {
    (qd.method != QdMethod::Qd1 && gravity_component(qd, pos, index).is_none())
        .then(|| gravity_shear_reason(index))
}

pub(crate) fn column_design_shear(
    qd: &SeismicQd,
    pos: f64,
    q_signed: f64,
    index: usize,
    sum_mu: f64,
) -> Result<f64, &'static str> {
    let qd1 = mechanism_shear(qd, sum_mu).unwrap_or(f64::INFINITY);
    if qd.method == QdMethod::Qd1 && qd1.is_finite() {
        return Ok(qd1);
    }
    let ql = gravity_component(qd, pos, index).ok_or_else(|| gravity_shear_reason(index))?;
    Ok(qd.method.resolve(qd1, qd2(ql, q_signed, qd.n_factor)))
}

fn mechanism_shear(qd: &SeismicQd, sum_mu: f64) -> Option<f64> {
    if qd.clear_length > 0.0 && sum_mu > 0.0 {
        let n_mech = if qd.n_mechanism > 0.0 {
            qd.n_mechanism
        } else {
            1.0
        };
        Some(n_mech * sum_mu / qd.clear_length)
    } else {
        None
    }
}

pub(super) fn beam_design_shear(
    qd: &SeismicQd,
    pos: f64,
    q_signed: f64,
    mechanism_issue: Option<&str>,
    sum_my_nmm: impl FnOnce() -> Result<f64, String>,
) -> Result<f64, String> {
    let ql_signed = qd
        .long_at
        .iter()
        .find(|(p, _)| (p - pos).abs() < 1e-6)
        .map(|(_, forces)| forces[1])
        .filter(|q| q.is_finite());
    let qd2 = |ql_signed| {
        if !qd.n_factor.is_finite() || qd.n_factor < 1.5 {
            return Err("QD2用の水平荷重時せん断力の割増係数は有限で1.5以上が必要です");
        }
        let demand_n = qd2(ql_signed, q_signed, qd.n_factor);
        demand_n
            .is_finite()
            .then_some(demand_n)
            .ok_or("QD2の設計用せん断力が非有限です")
    };
    let ql_signed = ql_signed.ok_or(gravity_shear_reason(1));
    if qd.method == QdMethod::Qd2 {
        return ql_signed.and_then(qd2).map_err(str::to_owned);
    }
    let qd1 = beam_qd1(qd, mechanism_issue, sum_my_nmm);
    match (qd1, ql_signed) {
        (Ok(qd1), Ok(_)) if qd.method == QdMethod::Qd1 => Ok(qd1),
        (Ok(qd1), Ok(ql)) => Ok(qd1.min(qd2(ql)?)),
        (Err(qd1_reason), Err(ql_reason)) => Err(format!("{qd1_reason}。{ql_reason}")),
        (Err(reason), Ok(_)) => Err(reason),
        (Ok(_), Err(reason)) => Err(reason.into()),
    }
}

fn beam_qd1(
    qd: &SeismicQd,
    mechanism_issue: Option<&str>,
    sum_my_nmm: impl FnOnce() -> Result<f64, String>,
) -> Result<f64, String> {
    let q0 = qd.q_simple.filter(|q| q.is_finite() && *q >= 0.0);
    if let Some(reason) = mechanism_issue {
        return if q0.is_none() {
            Err(format!("{reason}。{BEAM_Q0_ISSUE}"))
        } else {
            Err(reason.into())
        };
    }
    let q0 = q0.ok_or(BEAM_Q0_ISSUE)?;
    let sum_my_nmm = sum_my_nmm()?;
    if !qd.clear_length.is_finite() || qd.clear_length <= 0.0 {
        return Err("QD1用の内法長さは有限な正値でなければなりません".into());
    }
    if !qd.n_mechanism.is_finite() || qd.n_mechanism < 1.0 {
        return Err("QD1用のメカニズム側割増係数は有限で1.0以上が必要です".into());
    }
    let demand_n = q0 + qd.n_mechanism * sum_my_nmm / qd.clear_length;
    if !demand_n.is_finite() {
        return Err("QD1の設計用せん断力が非有限です".into());
    }
    Ok(demand_n)
}

const BEAM_Q0_ISSUE: &str = "QD1用の部材荷重由来Q0が不足または不正です。FEMのQLでは代替しません";

fn qd2(ql_signed: f64, q_signed: f64, n_factor: f64) -> f64 {
    ql_signed.abs() + n_factor * (q_signed - ql_signed).abs()
}

/// 地震時短期の設計用せん断力 QD [N]。`QD = min(QD1, QD2)`。
/// `ctx.seismic_qd` が None の場合は解析せん断力 `|q_signed|` を返す。
/// `q_index`: 長期内力配列のせん断成分位置（qy=1, qz=2）。
pub(crate) fn legacy_column_design_shear(
    ctx: &DesignCtx,
    pos: f64,
    q_signed: f64,
    q_index: usize,
    sum_mu: f64,
) -> f64 {
    let Some(qd) = &ctx.seismic_qd else {
        return q_signed.abs();
    };
    let Some(ql_signed) = qd
        .long_at
        .iter()
        .find(|(p, _)| (p - pos).abs() < 1e-6)
        .map(|(_, f)| f[q_index])
    else {
        return q_signed.abs();
    };
    let qd2 = qd2(ql_signed, q_signed, qd.n_factor);
    let qd1 = mechanism_shear(qd, sum_mu).unwrap_or(f64::INFINITY);
    qd.method.resolve(qd1, qd2)
}
