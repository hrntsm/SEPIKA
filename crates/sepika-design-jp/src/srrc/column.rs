//! 鉄骨鉄筋コンクリート造柱の許容応力度検定。
//!
//! 軸力＋二軸曲げ＋二方向せん断の複合検定を行う。曲げ耐力 MA(N) は
//! RC 部分の N-M 相関曲線（[`src_column_nm_curve`]）と鉄骨単体の許容曲げ
//! モーメントを軸力範囲に応じて 3 分岐で累加する（[`src_column_axis_ma`]）。
//! せん断は [`super::src_column_shear_check`] に委譲する。

use super::{
    ratio_or_large, src_column_shear_check, steel_h_props, SrcAxisProps, SrcShearResult,
    SrcShortShearDemand,
};
use crate::material_strength::main_rebar_grade;
use crate::rc::{
    concrete_allowable_compression, concrete_allowable_shear_class, interp_ma,
    rebar_allowable_shear, rebar_allowable_tension, young_ratio_n,
};
use crate::steel::{steel_fs, steel_ft};
use crate::{
    CheckComponent, CheckKind, CheckResult, DesignCtx, LoadTerm, MemberForcesAt, QdMethod,
    SeismicQd,
};
use sepika_core::model::Material;

/// 同一地震荷重組合せの両方向で、付1.4-1の適用を呼出側が確認した曲げ耐力。
/// Qyは強軸まわり、Qzは弱軸まわり。各方向の加力の向きに対応する値を指定する。
#[derive(Clone, Copy, Debug)]
pub struct SrcColumnSeismicBending {
    pub qy: SrcColumnSeismicBendingAxis,
    pub qz: SrcColumnSeismicBendingAxis,
}

/// 検討方向の柱頭・柱脚の材料別曲げ耐力と、柱頭RC曲げの適用条件。
/// 呼出側が両端それぞれの軸力、配筋、曲げ式および耐力の適用範囲を確認する。
#[derive(Clone, Copy, Debug)]
pub struct SrcColumnSeismicBendingAxis {
    pub head: SrcColumnSeismicBendingEnd,
    pub base: SrcColumnSeismicBendingEnd,
    pub head_limit: SrcColumnHeadLimit,
}

/// 部材端の材料別曲げ耐力の大きさ [N·mm]。有限な非負値を指定する。
#[derive(Clone, Copy, Debug)]
pub struct SrcColumnSeismicBendingEnd {
    /// 鉄骨部分の短期許容曲げ耐力。
    pub steel_allowable_n_mm: f64,
    /// RC部分の終局曲げ耐力。柱頭では梁による上限を適用する前の値。
    pub rc_ultimate_n_mm: f64,
}

/// 検討方向における柱頭RC曲げの適用条件。
#[derive(Clone, Copy, Debug)]
pub enum SrcColumnHeadLimit {
    /// 柱の両端が曲げ降伏することが明らかで、梁による制限を省ける場合。
    BothEndsYield,
    /// 接続する左右梁から柱頭のRC曲げ耐力を制限する場合。
    ConnectedBeams {
        /// 左右梁の鉄骨短期許容曲げ耐力とRC終局曲げ耐力の合計 [N·mm]。
        /// 梁RC部分には根拠を確認した短期許容曲げ耐力も使用できる。
        sum_n_mm: f64,
        top_story: bool,
    },
}

impl SrcColumnSeismicBendingAxis {
    fn shear_demand_n(
        &self,
        height_mm: f64,
        main_rebar_valid: bool,
    ) -> Result<SrcShortShearDemand, &'static str> {
        let steel_head = self.head.steel_allowable_n_mm;
        let steel_base = self.base.steel_allowable_n_mm;
        if ![steel_head, steel_base]
            .iter()
            .all(|m| m.is_finite() && *m >= 0.0)
        {
            return Err("柱頭・柱脚の材料別曲げ耐力には有限な非負値が必要です");
        }

        let rc_sum = if main_rebar_valid {
            let rc_head = self.head.rc_ultimate_n_mm;
            let rc_base = self.base.rc_ultimate_n_mm;
            if ![rc_head, rc_base]
                .iter()
                .all(|m| m.is_finite() && *m >= 0.0)
            {
                return Err("柱頭・柱脚の材料別曲げ耐力には有限な非負値が必要です");
            }
            let rc_head = match self.head_limit {
                SrcColumnHeadLimit::BothEndsYield => rc_head,
                SrcColumnHeadLimit::ConnectedBeams {
                    sum_n_mm,
                    top_story,
                } => {
                    if !sum_n_mm.is_finite() || sum_n_mm < 0.0 {
                        return Err("柱頭の接続梁の材料別曲げ耐力の合計には有限な非負値が必要です");
                    }
                    let factor = if top_story { 1.0 } else { 0.5 };
                    let rc_limit = factor * sum_n_mm - steel_head;
                    if rc_limit < 0.0 {
                        return Err("柱頭の接続梁によるRC曲げ耐力の上限が負となり、適用範囲外です");
                    }
                    rc_head.min(rc_limit)
                }
            };
            Some(rc_head + rc_base)
        } else {
            None
        };

        let steel_sum = steel_head + steel_base;
        let s_qd = steel_sum / height_mm;
        let r_qd = rc_sum.map(|sum| sum / height_mm);
        if !s_qd.is_finite() || r_qd.is_some_and(|qd| !qd.is_finite()) {
            return Err("材料別の設計用せん断力が非有限です");
        }
        if (steel_sum > 0.0 && s_qd == 0.0)
            || rc_sum
                .zip(r_qd)
                .is_some_and(|(sum, qd)| sum > 0.0 && qd == 0.0)
        {
            return Err("材料別の設計用せん断力が数値範囲外です");
        }
        Ok(SrcShortShearDemand {
            steel_n: s_qd,
            rc_n: r_qd,
        })
    }
}

pub(super) fn gravity_reference_issue(
    pos: f64,
    qd: Option<&SeismicQd>,
    missing_gravity: bool,
) -> Option<&'static str> {
    if missing_gravity {
        return Some("設計用せん断力に必要な重力参照内力が不足しています");
    }
    let qd = qd?;
    (!pos.is_finite()
        || !qd
            .long_at
            .iter()
            .find(|(ref_pos, _)| (ref_pos - pos).abs() < 1e-6)
            .is_some_and(|(_, values)| values[1].is_finite() && values[2].is_finite()))
    .then_some("設計用せん断力に必要な同位置の重力参照せん断力QL（Qy・Qz）が不足または非有限です")
}

struct ColumnSeismicDemand {
    axes: [Option<SrcShortShearDemand>; 2],
    issues: Vec<String>,
}

fn src_column_seismic_qd(
    forces: &MemberForcesAt,
    ctx: &DesignCtx,
    missing_gravity: bool,
    rc_demand_needed: [bool; 2],
    shear_rebar_valid: bool,
) -> Result<Option<ColumnSeismicDemand>, String> {
    if ctx.term != LoadTerm::Short {
        return Ok(None);
    }
    if let Some(reason) =
        gravity_reference_issue(forces.pos, ctx.seismic_qd.as_ref(), missing_gravity)
    {
        return Err(reason.into());
    }
    let Some(qd) = &ctx.seismic_qd else {
        return Ok(None);
    };
    let missing_bending = "曲げ方式（Qd1）には、柱頭・柱脚の軸力と方向別の配筋・曲げ式に基づく材料別曲げ耐力、および柱頭の接続梁による上限または両端曲げ降伏の確認が必要です";
    let missing_alternative =
        "代替式 2(Q−Qs) に必要な一次設計用地震力による柱せん断力Qを評価できません";
    if qd.method == QdMethod::Qd2 {
        return Err(format!("Qd2: {missing_alternative}"));
    }
    if qd.method == QdMethod::Min {
        return Err(if ctx.src_column_seismic_bending.is_none() {
            format!("Min: {missing_alternative}。{missing_bending}")
        } else {
            format!("Min: {missing_alternative}")
        });
    }
    let bending = ctx.src_column_seismic_bending.ok_or(missing_bending)?;
    if !qd.clear_length.is_finite() || qd.clear_length <= 0.0 {
        return Err("曲げ方式（Qd1）には有限で正の内法高さが必要です".into());
    }
    if !forces.qy.is_finite() || !forces.qz.is_finite() {
        return Err("組合せのせん断力（Qy・Qz）が非有限です".into());
    }
    let mut demand = ColumnSeismicDemand {
        axes: [None, None],
        issues: Vec::new(),
    };
    for (index, (direction, axis)) in [("Qy", bending.qy), ("Qz", bending.qz)]
        .into_iter()
        .enumerate()
    {
        match axis.shear_demand_n(qd.clear_length, rc_demand_needed[index]) {
            Ok(axis_demand) => demand.axes[index] = Some(axis_demand),
            Err(reason) if shear_rebar_valid => return Err(format!("{direction}方向の{reason}")),
            Err(reason) => {
                demand.issues.push(format!("{direction}方向の{reason}"));
                demand.axes[index] = axis.shear_demand_n(qd.clear_length, false).ok();
            }
        }
    }
    Ok(Some(demand))
}

struct SrcColumnAxis {
    props: SrcAxisProps,
    /// 直交方向の主筋総断面積（断面中央に集約、RC 規準 14条の慣習）。
    at_perp: f64,
    /// 当該軸の主筋径に応じた許容引張・圧縮応力度。
    ft: f64,
}

/// 中立軸位置 xn における RC 部分の (N, |M|) を求める（`rc/column.rs` の
/// `column_nm_at_xn` と同じ考え方。コンクリート許容応力度に `fc_prime`
/// （鉄骨フランジ食い込みによる低減後）を用いる点のみ異なる）。
fn src_column_nm_at_xn(
    axis: &SrcColumnAxis,
    fc_prime: f64,
    n_ratio: f64,
    xn: f64,
) -> Option<(f64, f64)> {
    if xn <= 0.0 {
        return None;
    }
    let p = &axis.props;
    let d_full = p.d_full;
    let b = p.b;
    let r_fc = axis.ft;
    let ft = axis.ft;

    let s_bar = |y: f64, area: f64| -> f64 {
        if area <= 0.0 {
            return f64::INFINITY;
        }
        let diff = xn - y;
        if diff.abs() < 1e-9 {
            return f64::INFINITY;
        }
        if diff > 0.0 {
            r_fc / (n_ratio * diff)
        } else {
            ft / (n_ratio * (-diff))
        }
    };

    let s1 = fc_prime / xn;
    let s2 = s_bar(p.dt, p.ac);
    let s3 = s_bar(d_full - p.dt, p.at);
    let s = s1.min(s2).min(s3);
    if !s.is_finite() || s <= 0.0 {
        return None;
    }

    let xc = xn.min(d_full);
    if xc <= 0.0 {
        return None;
    }

    let nc = b * s * (xn * xc - xc * xc / 2.0);
    let mc =
        b * s * (xn * (d_full / 2.0) * xc - (xn + d_full / 2.0) * xc * xc / 2.0 + xc.powi(3) / 3.0);

    let bar_contrib = |y: f64, area: f64| -> (f64, f64) {
        if area <= 0.0 {
            return (0.0, 0.0);
        }
        let mult = if y <= xn { n_ratio - 1.0 } else { n_ratio };
        let force = area * mult * s * (xn - y);
        let moment = force * (d_full / 2.0 - y);
        (force, moment)
    };

    let (n_c, m_c) = bar_contrib(p.dt, p.ac);
    let (n_t, m_t) = bar_contrib(d_full - p.dt, p.at);
    let (n_p, m_p) = bar_contrib(d_full / 2.0, axis.at_perp);

    let n_total = nc + n_c + n_t + n_p;
    let m_total = mc + m_c + m_t + m_p;
    Some((n_total, m_total.abs()))
}

const SRC_XN_SCAN_POINTS: usize = 300;
const SRC_XN_RATIO_MIN: f64 = 0.02;
const SRC_XN_RATIO_MAX: f64 = 10.0;

/// N-M 相関曲線（RC 部分のみ）を xn/D の対数スキャンで構成する。
///
/// 圧縮側は `(rnc, 0)`、引張側は `(rnt, 0)` をアンカーとして追加する。
/// 引張側アンカーがないと、xn スキャン最小点より引張側で許容曲げが
/// 頭打ちクランプされ、純引張耐力 rNt 近傍の rM を過大評価する
/// （正しくは N → rNt で rM → 0 に収束する）。
fn src_column_nm_curve(
    axis: &SrcColumnAxis,
    fc_prime: f64,
    n_ratio: f64,
    rnc: f64,
    rnt: f64,
) -> Vec<(f64, f64)> {
    let mut pts = Vec::with_capacity(SRC_XN_SCAN_POINTS + 2);
    let log_min = SRC_XN_RATIO_MIN.ln();
    let log_max = SRC_XN_RATIO_MAX.ln();
    for i in 0..SRC_XN_SCAN_POINTS {
        let t = i as f64 / (SRC_XN_SCAN_POINTS as f64 - 1.0);
        let ratio = (log_min + t * (log_max - log_min)).exp();
        let xn = axis.props.d_full * ratio;
        if let Some(pt) = src_column_nm_at_xn(axis, fc_prime, n_ratio, xn) {
            if pt.0.is_finite() && pt.1.is_finite() && pt.0 >= rnt {
                pts.push(pt);
            }
        }
    }
    pts.push((rnc, 0.0));
    pts.push((rnt, 0.0));
    pts.sort_by(|a, b| a.0.total_cmp(&b.0));
    pts
}

/// SRC 柱 1 軸分の許容曲げモーメント MA(N)。
/// （RC+鉄骨累加 / 圧縮超過で鉄骨のみ / 引張超過で鉄骨のみ）を実装する。
#[allow(clippy::too_many_arguments)]
fn src_column_axis_ma(
    n_design: f64,
    rnc: f64,
    rnt: f64,
    sa: f64,
    s_ft: f64,
    s_fc: f64,
    sz: f64,
    s_mo: f64,
    curve: &[(f64, f64)],
) -> f64 {
    if n_design >= rnt && n_design <= rnc {
        s_mo + interp_ma(curve, n_design)
    } else if n_design > rnc {
        let sn = n_design - rnc;
        (sz * (s_fc - sn / sa)).max(0.0)
    } else {
        let sn = n_design - rnt;
        (sz * (s_ft - sn.abs() / sa)).max(0.0)
    }
}

/// SRC 柱の軸力・一軸曲げと方向別せん断を検定する。
///
/// `props_z`・`props_y` は強軸・弱軸の RC 部分諸元、`at_perp_z`・`at_perp_y` は
/// 直交方向の主筋総断面積、`as_total` は全主筋断面積。`main_dia_z`・`main_dia_y`
/// は各軸の主筋径。
#[allow(clippy::too_many_arguments)]
pub(crate) fn src_column_check(
    forces: &MemberForcesAt,
    mat: &Material,
    ctx: &DesignCtx,
    props_z: SrcAxisProps,
    props_y: SrcAxisProps,
    at_perp_z: f64,
    at_perp_y: f64,
    as_total: f64,
    main_dia_z: f64,
    main_dia_y: f64,
    steel_height: f64,
    steel_width: f64,
    steel_web_thick: f64,
    steel_flange_thick: f64,
    f_value: f64,
    fc_raw: f64,
    missing_gravity: bool,
    ordinary_shear_issue: Option<&str>,
    main_rebar_valid: bool,
    shear_rebar_valid: bool,
) -> (CheckResult, Option<String>) {
    let long_term = ctx.term == LoadTerm::Long;

    let fc_allow = concrete_allowable_compression(fc_raw, long_term);
    let fs = concrete_allowable_shear_class(fc_raw, mat.concrete_class, long_term);
    let n_ratio = young_ratio_n(fc_raw);
    let w_ft = ctx
        .shear_rebar_material
        .as_ref()
        .filter(|_| shear_rebar_valid && !long_term)
        .map(|rebar| {
            rebar_allowable_shear(
                crate::material_strength::shear_rebar_grade(Some(rebar)),
                false,
            )
        });

    let s_ft = steel_ft(f_value, ctx.term);
    let s_fs = steel_fs(f_value, ctx.term);
    let (sa, sz_z, sz_y) = steel_h_props(
        steel_height,
        steel_width,
        steel_web_thick,
        steel_flange_thick,
    );

    let short_seismic = !long_term && (missing_gravity || ctx.seismic_qd.is_some());
    let n_design = -forces.n;
    let seismic_limit =
        short_seismic.then(|| props_z.b * props_z.d_full * fc_raw / 3.0 + 2.0 * sa * s_ft / 3.0);
    let seismic_ratio = seismic_limit.map(|limit| n_design.max(0.0) / limit);
    let invalid_z = !forces.mz.is_finite();
    let invalid_y = !forces.my.is_finite();
    let simultaneous_bending = !invalid_z && !invalid_y && forces.mz != 0.0 && forces.my != 0.0;
    let (lk_y, lk_z) = crate::steel::buckling_lengths_with_defaults(
        crate::MemberKind::Column,
        ctx.node_span_mm.unwrap_or(ctx.length),
        ctx.lk_y,
        ctx.lk_z,
    );
    let outside_short_column = [(lk_y, props_z.d_full), (lk_z, props_y.d_full)]
        .into_iter()
        .any(|(mm, depth_mm)| !mm.is_finite() || mm <= 0.0 || mm > 12.0 * depth_mm);
    let mut invalid_ratio = false;
    let mut short_demands = [None; 2];
    let mut result = CheckResult {
        basis: "SRC柱".into(),
        detail: String::new(),
        components: Vec::new(),
    };
    if main_rebar_valid && !outside_short_column && !simultaneous_bending {
        let grade = main_rebar_grade(ctx.rebar_material.as_ref());
        let s_fc = s_ft;
        let s_pc = (steel_width * steel_flange_thick) / (props_z.b * props_z.d_full).max(1e-9);
        let fc_prime = (fc_allow * (1.0 - 15.0 * s_pc)).max(0.0);

        let ft_z = rebar_allowable_tension(grade, main_dia_z, long_term);
        let ft_y = rebar_allowable_tension(grade, main_dia_y, long_term);
        let ft_axial = rebar_allowable_tension(grade, main_dia_z.max(main_dia_y), long_term);

        let ae = props_z.b * props_z.d_full + (n_ratio - 1.0) * as_total;
        let rnc1 = ae * fc_prime;
        let rnc2 = ae * ft_axial / n_ratio;
        let rnc = rnc1.min(rnc2).max(0.0);
        let rnt = -(as_total * ft_axial);

        let s_nc = sa * s_fc;
        let s_nt = sa * s_ft;

        let axis_z = SrcColumnAxis {
            props: props_z,
            at_perp: at_perp_z,
            ft: ft_z,
        };
        let axis_y = SrcColumnAxis {
            props: props_y,
            at_perp: at_perp_y,
            ft: ft_y,
        };

        let curve_z = src_column_nm_curve(&axis_z, fc_prime, n_ratio, rnc, rnt);
        let curve_y = src_column_nm_curve(&axis_y, fc_prime, n_ratio, rnc, rnt);

        let s_mo_z = sz_z * s_ft;
        let s_mo_y = sz_y * s_ft;

        let ma_z = src_column_axis_ma(n_design, rnc, rnt, sa, s_ft, s_fc, sz_z, s_mo_z, &curve_z);
        let ma_y = src_column_axis_ma(n_design, rnc, rnt, sa, s_ft, s_fc, sz_y, s_mo_y, &curve_y);
        if !long_term
            && !short_seismic
            && ordinary_shear_issue.is_none()
            && (rnt..=rnc).contains(&n_design)
        {
            short_demands = [
                (forces.qy, forces.mz, s_mo_z, ma_z),
                (forces.qz, forces.my, s_mo_y, ma_y),
            ]
            .map(|(q, m, steel_ma, ma)| {
                let m = m.abs();
                if !q.is_finite() || m <= 0.0 || !(steel_ma..=ma).contains(&m) {
                    return None;
                }
                Some(SrcShortShearDemand {
                    steel_n: q.abs() * (steel_ma / m),
                    rc_n: Some(q.abs() * ((m - steel_ma) / m)),
                })
            });
        }

        let ratio_z = (!invalid_z).then(|| ratio_or_large(forces.mz, ma_z));
        let ratio_y = (!invalid_y).then(|| ratio_or_large(forces.my, ma_y));
        let ratio_bending = ratio_z.unwrap_or(0.0).max(ratio_y.unwrap_or(0.0));

        let mut ratio_axial = if n_design > rnc + s_nc {
            n_design / (rnc + s_nc)
        } else if n_design < rnt - s_nt {
            (-n_design) / (-rnt + s_nt)
        } else {
            0.0
        };
        if let Some(seismic_ratio) = seismic_ratio {
            ratio_axial = ratio_axial.max(seismic_ratio);
        }

        invalid_ratio = short_seismic && (!ratio_axial.is_finite() || !ratio_bending.is_finite());
        let ratio = if invalid_ratio {
            [ratio_axial, ratio_bending]
                .into_iter()
                .chain(ratio_z)
                .chain(ratio_y)
                .filter(|ratio| ratio.is_finite())
                .fold(0.0, f64::max)
        } else {
            ratio_axial.max(ratio_bending)
        };
        result.basis.push_str(": 軸力・一軸曲げ（17条）");
        result.components.push(CheckComponent {
            kind: CheckKind::AxialBending,
            ratio,
            detail: format!(
                "rNc={:.1} N, rNt={:.1} N, sNc={:.1} N, sNt={:.1} N, N={:.1} N, \
                 MAz={:.1} N·mm, MAy={:.1} N·mm, mz={:.1} N·mm, my={:.1} N·mm, s_pc={:.5}, fc'={:.3}",
                rnc, rnt, s_nc, s_nt, n_design, ma_z, ma_y, forces.mz, forces.my, s_pc, fc_prime
            ),
    });
    } else if let (Some(limit), Some(ratio)) = (seismic_limit, seismic_ratio) {
        result.basis.push_str(": 地震時圧縮力上限（17.30）");
        result.components.push(CheckComponent {
            kind: CheckKind::Axial,
            ratio,
            detail: format!("N={n_design:.1} N, Nl={limit:.1} N"),
        });
    }
    let incomplete_reason = |shear_reason: Option<&str>| {
        let reason = [
            invalid_z.then_some("組合せの曲げモーメントMzが非有限です"),
            invalid_y.then_some("組合せの曲げモーメントMyが非有限です"),
            invalid_ratio.then_some("軸力・曲げの検定比が非有限です"),
            (main_rebar_valid && outside_short_column).then_some(
                "軸力・一軸曲げの算定には、各方向の断面せいの12倍以下で正の座屈長さが必要です",
            ),
            (main_rebar_valid && simultaneous_bending)
                .then_some("二軸同時の曲げは、軸力・一軸曲げの算定の適用範囲外です"),
            shear_reason,
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("。");
        (!reason.is_empty()).then(|| format!("SRC柱検定: {reason}"))
    };
    if ordinary_shear_issue.is_some() {
        return (result, incomplete_reason(ordinary_shear_issue));
    }
    let rc_demand_needed =
        [props_z.pw, props_y.pw].map(|pw| main_rebar_valid && (shear_rebar_valid || pw == 0.0));
    let demand_n = match src_column_seismic_qd(
        forces,
        ctx,
        missing_gravity,
        rc_demand_needed,
        shear_rebar_valid,
    ) {
        Ok(demand_n) => demand_n,
        Err(reason) => return (result, incomplete_reason(Some(&reason))),
    };

    let steel_to_concrete_young = if long_term {
        let ratio = ctx.steel_material.as_ref().and_then(|steel| {
            (steel.young.is_finite()
                && steel.young > 0.0
                && mat.young.is_finite()
                && mat.young > 0.0)
                .then(|| steel.young / mat.young)
                .filter(|ratio| ratio.is_finite() && *ratio > 0.0)
        });
        let Some(ratio) = ratio else {
            return (
                result,
                incomplete_reason(Some(
                    "長期せん断には鉄骨とコンクリートの正で有限なヤング係数比が必要です",
                )),
            );
        };
        ratio
    } else {
        0.0
    };
    let web_clear_depth_mm = steel_height - 2.0 * steel_flange_thick;
    let beta_z = if props_z.b * props_z.j > 1e-9 {
        steel_to_concrete_young * steel_web_thick * web_clear_depth_mm / (props_z.b * props_z.j)
    } else {
        0.0
    };
    let beta_y = if props_y.b * props_y.j > 1e-9 {
        1.33 * steel_to_concrete_young * steel_width * steel_flange_thick / (props_y.b * props_y.j)
    } else {
        0.0
    };

    let (m_alpha_z, q_alpha_z) = ctx.shear_span.unwrap_or((forces.mz.abs(), forces.qy.abs()));
    let b_prime_z = (props_z.b - steel_width).max(0.0);
    let shear_z = src_column_shear_check(
        forces.qy,
        m_alpha_z,
        q_alpha_z,
        props_z.j,
        props_z.d,
        props_z.b,
        b_prime_z,
        props_z.pw,
        fs,
        w_ft,
        s_fs,
        steel_web_thick * web_clear_depth_mm,
        2.0,
        beta_z,
        demand_n.as_ref().map_or(short_demands[0], |d| d.axes[0]),
        ctx.term,
    );

    let (m_alpha_y, q_alpha_y) = ctx
        .shear_span_y
        .unwrap_or((forces.my.abs(), forces.qz.abs()));
    let b_prime_y = (props_z.d_full - steel_height).max(0.0);
    let shear_y = src_column_shear_check(
        forces.qz,
        m_alpha_y,
        q_alpha_y,
        props_y.j,
        props_y.d,
        props_y.b,
        b_prime_y,
        props_y.pw,
        fs,
        w_ft,
        s_fs,
        (4.0 / 3.0) * steel_width * steel_flange_thick,
        2.0,
        beta_y,
        demand_n.as_ref().map_or(short_demands[1], |d| d.axes[1]),
        ctx.term,
    );

    let mut shear_issue = None;
    let (ratio, detail) = if let Some(mut demand) = demand_n {
        let mut results = Vec::new();
        for (direction, shear) in [("Qy", shear_z.as_ref()), ("Qz", shear_y.as_ref())] {
            let Some(shear) = shear else { continue };
            match seismic_shear_detail(direction, shear, true) {
                Ok(value) => results.push(value),
                Err(reason) if shear_rebar_valid => {
                    return (result, incomplete_reason(Some(&reason)))
                }
                Err(reason) => {
                    demand.issues.push(reason);
                    results.extend(seismic_shear_detail(direction, shear, false).ok());
                }
            }
        }
        shear_issue = (!demand.issues.is_empty()).then(|| demand.issues.join("。"));
        let Some(ratio) = results.iter().map(|(ratio, _)| *ratio).reduce(f64::max) else {
            return (result, incomplete_reason(shear_issue.as_deref()));
        };
        result.basis.push_str(" + 地震時せん断曲げ方式（付1.4-1）");
        let detail = results
            .into_iter()
            .map(|(_, detail)| detail)
            .collect::<Vec<_>>()
            .join(", ");
        (ratio, detail)
    } else if !long_term {
        let mut ratios = Vec::new();
        let mut details = Vec::new();
        let mut issues = Vec::new();
        for (direction, q, shear) in [("Qy", forces.qy, shear_z), ("Qz", forces.qz, shear_y)] {
            if q == 0.0 {
                continue;
            }
            let Some(shear) = shear else {
                issues.push(format!(
                    "{direction}方向の18条のせん断力の分担に必要な材料別の負担曲げを確定できません"
                ));
                continue;
            };
            let steel_ratio = ratio_or_large(shear.s_q, shear.s_qa);
            let mut detail = format!(
                "{direction}方向 sQ={:.1} N, sQA={:.1} N",
                shear.s_q, shear.s_qa
            );
            let ratio = if let Some((q, qa)) = shear.r_q.zip(shear.r_qa) {
                detail.push_str(&format!(", rQ={q:.1} N, rQA={qa:.1} N"));
                steel_ratio.max(ratio_or_large(q, qa))
            } else {
                steel_ratio
            };
            ratios.push(ratio);
            details.push(detail);
        }
        shear_issue = (!issues.is_empty()).then(|| issues.join("。"));
        let Some(ratio) = ratios.into_iter().reduce(f64::max) else {
            return (result, incomplete_reason(shear_issue.as_deref()));
        };
        result.basis.push_str(" + 短期せん断（18条）");
        (ratio, details.join(", "))
    } else if let (Some(shear_z), Some(shear_y)) = (shear_z, shear_y) {
        result.basis.push_str(" + 長期せん断併用式");
        (
            shear_z.ratio.max(shear_y.ratio),
            format!(
                "QA強軸={:.1} N、QA弱軸={:.1} N",
                shear_z.r_qa.unwrap(),
                shear_y.r_qa.unwrap(),
            ),
        )
    } else {
        return (
            result,
            incomplete_reason(long_term.then_some("長期せん断の要求値または許容値が非有限です")),
        );
    };
    result.components.push(CheckComponent {
        kind: CheckKind::Shear,
        ratio,
        detail,
    });
    (result, incomplete_reason(shear_issue.as_deref()))
}

fn seismic_shear_detail(
    direction: &str,
    shear: &SrcShearResult,
    include_rc: bool,
) -> Result<(f64, String), String> {
    let rc_demand = shear.r_q.filter(|_| include_rc);
    let rc = rc_demand.zip(shear.r_qa);
    if ![Some(shear.s_qa), rc.map(|(_, qa)| qa)]
        .iter()
        .flatten()
        .all(|qa| qa.is_finite() && *qa > 0.0)
    {
        return Err(format!(
            "{direction}方向の材料別許容せん断力には有限な正値が必要です"
        ));
    }
    let steel_ratio = shear.s_q / shear.s_qa;
    let rc_ratio = rc.map(|(q, qa)| q / qa);
    if !steel_ratio.is_finite() || rc_ratio.is_some_and(|ratio| !ratio.is_finite()) {
        return Err(format!("{direction}方向の材料別せん断検定比が非有限です"));
    }
    if (shear.s_q > 0.0 && steel_ratio == 0.0)
        || rc_demand
            .zip(rc_ratio)
            .is_some_and(|(q, ratio)| q > 0.0 && ratio == 0.0)
    {
        return Err(format!(
            "{direction}方向の材料別せん断検定比が数値範囲外です"
        ));
    }
    let rc_detail = match (rc_demand, shear.r_qa, rc_ratio) {
        (Some(q), Some(qa), Some(ratio)) => {
            format!("RC rQD={q:e} N, rQA={qa:e} N, rQD/rQA={ratio:e}")
        }
        (Some(q), None, _) => format!("RC rQD={q:e} N, rQA=未算定"),
        _ => "RC rQD=未算定".into(),
    };
    let detail = format!(
        "{direction}方向 鉄骨 sQD={:e} N, sQA={:e} N, sQD/sQA={steel_ratio:e}, {rc_detail}",
        shear.s_q, shear.s_qa,
    );
    Ok((
        rc_ratio.map_or(steel_ratio, |ratio| steel_ratio.max(ratio)),
        detail,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::srrc::tests::{
        ctx_column, make_material, make_section, src_column_shape, zero_forces,
    };
    use crate::steel::steel_f_value_prefix;
    use crate::DesignCheck;

    #[test]
    fn src_column_short_shear_uses_a_confirmed_uniaxial_bending_split() {
        let sec = make_section(src_column_shape());
        let mat = make_material(24.0, "Fc24");
        let ctx = ctx_column(LoadTerm::Short);
        let forces = MemberForcesAt {
            mz: 250.0e6,
            qy: 500.0e3,
            ..zero_forces()
        };
        let result = crate::SrcDesign
            .check(&forces, &sec, &mat, &ctx)
            .unwrap_checked();
        let shear = result
            .components
            .iter()
            .find(|row| row.kind == CheckKind::Shear)
            .unwrap();
        // 矩形積分によるsMA=203.1944775 kN·m、sQA=332.1380629 kN。
        assert!(
            (shear.ratio - 1.223_554_300_050_953).abs() < 1e-9,
            "{shear:?}"
        );
        let time_history: Vec<_> = crate::SrcDesign
            .check_time_history_outcomes(&forces, &sec, &mat, &ctx, false)
            .collect();
        assert!(
            matches!(time_history.as_slice(), [crate::CheckOutcome::Skipped { reason }] if reason.contains("時刻歴の地震内力")),
            "{time_history:?}"
        );
        let pure_shear = MemberForcesAt { mz: 0.0, ..forces };
        let outcome = crate::SrcDesign.check(&pure_shear, &sec, &mat, &ctx);
        assert!(
            matches!(outcome, crate::CheckOutcome::Skipped { reason } if reason.contains("材料別の負担曲げを確定できません"))
        );
    }
    use sepika_core::units::ConcreteClass;

    /// components に AxialBending・Shear が入ることを確認する。
    #[test]
    fn test_src_column_components_axial_bending_and_shear() {
        let shape = src_column_shape();
        let sec = make_section(shape);
        let mat = make_material(24.0, "SD345");
        let ctx = ctx_column(LoadTerm::Long);
        let forces = MemberForcesAt {
            n: -500_000.0,
            qy: 30_000.0,
            qz: 20_000.0,
            mz: 8_000_000.0,
            ..zero_forces()
        };
        let design = crate::SrcDesign;
        let result = design.check(&forces, &sec, &mat, &ctx).unwrap_checked();
        assert_eq!(result.components.len(), 2);
        assert!(result
            .components
            .iter()
            .any(|c| c.kind == crate::CheckKind::AxialBending));
        assert!(result
            .components
            .iter()
            .any(|c| c.kind == crate::CheckKind::Shear));
    }

    #[test]
    fn test_src_column_beyond_rnc_uses_steel_only() {
        let shape = src_column_shape();
        let sec = make_section(shape.clone());
        let mat = make_material(24.0, "SD345");
        let ctx = ctx_column(LoadTerm::Long);

        let forces = MemberForcesAt {
            n: -30_000_000.0,
            mz: 10_000_000.0,
            ..zero_forces()
        };
        let design = crate::SrcDesign;
        let result = design.check(&forces, &sec, &mat, &ctx).unwrap_checked();
        assert!(result.ratio().is_finite());
        assert!(crate::full_detail(&result).contains("rNc"));
        let _ = shape;
    }

    #[test]
    fn test_src_column_tension_side() {
        let shape = src_column_shape();
        let sec = make_section(shape);
        let mat = make_material(24.0, "SD345");
        let ctx = ctx_column(LoadTerm::Long);

        let forces = MemberForcesAt {
            n: 5_000_000.0,
            mz: 1_000_000.0,
            ..zero_forces()
        };
        let design = crate::SrcDesign;
        let result = design.check(&forces, &sec, &mat, &ctx).unwrap_checked();
        assert!(result.ratio().is_finite());
        assert!(result.ratio() > 0.0);
    }

    #[test]
    fn src_single_axis_capacity_is_used_for_one_axis_bending() {
        let shape = src_column_shape();
        let sec = make_section(shape);
        let mat = make_material(24.0, "SD345");
        let ctx = ctx_column(LoadTerm::Long);
        let design = crate::SrcDesign;

        let forces_z = MemberForcesAt {
            mz: 1.0,
            ..zero_forces()
        };
        let r0 = design.check(&forces_z, &sec, &mat, &ctx).unwrap_checked();
        let ma_z = 1.0 / r0.ratio();
        assert!(ma_z > 0.0 && ma_z.is_finite());

        let (_sa, sz_z, _) = steel_h_props(300.0, 200.0, 9.0, 14.0);
        let f_value = steel_f_value_prefix("SN400B", 14.0).unwrap();
        let s_ft = steel_ft(f_value, LoadTerm::Long);
        let s_mo = sz_z * s_ft;
        assert!(ma_z >= s_mo * 0.99, "ma_z={ma_z}, s_mo={s_mo}");

        let mz_test = ma_z * 0.3;
        let forces = MemberForcesAt {
            mz: mz_test,
            ..zero_forces()
        };
        let r = design.check(&forces, &sec, &mat, &ctx).unwrap_checked();
        assert!(
            (r.ratio() - 0.3).abs() < 0.05,
            "mz 単独 0.3 割合のとき ratio ≒ 0.3 のはず: ratio={}",
            r.ratio()
        );
    }

    /// SRC 柱でも軽量コンクリートの 0.9 倍低減は許容せん断応力度のみに適用され、
    /// rNc（RC 部分の許容圧縮）は普通コンクリートと一致する。
    #[test]
    fn test_src_column_lightweight_compression_matches_normal() {
        let shape = src_column_shape();
        let sec = make_section(shape);
        let mut mat_n = make_material(24.0, "SD345");
        mat_n.concrete_class = ConcreteClass::Normal;
        let mut mat_l = make_material(24.0, "SD345");
        mat_l.concrete_class = ConcreteClass::Lightweight1;
        let ctx = ctx_column(LoadTerm::Long);
        let design = crate::SrcDesign;

        let forces = MemberForcesAt {
            n: -50_000_000.0,
            ..zero_forces()
        };
        let r_n = design.check(&forces, &sec, &mat_n, &ctx).unwrap_checked();
        let r_l = design.check(&forces, &sec, &mat_l, &ctx).unwrap_checked();
        assert!(
            (r_l.ratio() - r_n.ratio()).abs() < 1e-12,
            "軽量1種でも rNc は普通と同じはず: normal={}, light={}",
            r_n.ratio(),
            r_l.ratio()
        );
    }
}
