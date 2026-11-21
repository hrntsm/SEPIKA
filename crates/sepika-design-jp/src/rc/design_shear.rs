//! 地震時短期の設計用せん断力 QD = min(QD1, QD2)。

use crate::{DesignCtx, QdMethod};

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
    let n_mech = if qd.n_mechanism > 0.0 {
        qd.n_mechanism
    } else {
        1.0
    };
    let qd1 = if qd.clear_length > 0.0 && sum_mu > 0.0 {
        let mech = n_mech * sum_mu / qd.clear_length;
        if is_column {
            mech
        } else {
            let q0 = qd.q_simple.unwrap_or(ql).abs();
            q0 + mech
        }
    } else {
        f64::INFINITY
    };
    qd.method.resolve(qd1, qd2)
}
