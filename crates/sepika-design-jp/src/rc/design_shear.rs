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

fn qd2(ql_signed: f64, q_signed: f64, n_factor: f64) -> f64 {
    ql_signed.abs() + n_factor * (q_signed - ql_signed).abs()
}

/// 地震時短期の設計用せん断力 QD [N]。`QD = min(QD1, QD2)`。
/// `ctx.seismic_qd` が None の場合は解析せん断力 `|q_signed|` を返す。
/// `q_index`: 長期内力配列のせん断成分位置（qy=1, qz=2）。
pub(crate) fn seismic_design_shear(
    ctx: &DesignCtx,
    pos: f64,
    q_signed: f64,
    q_index: usize,
    sum_mu: f64,
    is_column: bool,
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
    let ql = ql_signed.abs();
    let qd2 = qd2(ql_signed, q_signed, qd.n_factor);
    let qd1 = mechanism_shear(qd, sum_mu)
        .map(|mech| {
            if is_column {
                mech
            } else {
                qd.q_simple.unwrap_or(ql).abs() + mech
            }
        })
        .unwrap_or(f64::INFINITY);
    qd.method.resolve(qd1, qd2)
}
