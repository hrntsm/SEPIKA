//! SRC 造の断面検定（許容応力度検定。SRC 規準 1987 の
//! SRC 梁・SRC 柱部分に準拠）。
//!
//! - `beam`: 鉄骨鉄筋コンクリート造梁の断面検定（累加強度式 MA=sMo+rMA）。
//! - [`column`][]: 鉄骨鉄筋コンクリート造柱の断面検定（累加強度式・fc′低減）。
//! - [`panel_zone`][]: SRC 造柱梁接合部（パネルゾーン）の断面検定（SRC 規準）。

use crate::{CheckOutcome, DesignCheck, DesignCtx, LoadTerm, MemberForcesAt, MemberKind};
use sepika_core::model::{Material, Section};
use sepika_core::rc_rebar_geom::RectEdge;
use sepika_core::section_shape::{RcBeamRebar, RcRectColumnRebar, SectionShape};

mod beam;
mod column;
pub mod panel_zone;

pub(crate) use crate::ratio_or_large;
pub(crate) use crate::rc::{shear_alpha, AxisProps as SrcAxisProps};
pub use column::{
    SrcColumnHeadLimit, SrcColumnSeismicBending, SrcColumnSeismicBendingAxis,
    SrcColumnSeismicBendingEnd,
};

/// 内蔵鋼材の断面積・断面係数を [`SectionShape`] の断面性能計算を借りて
/// 求める（H 形鋼: `sA`, 強軸 `sZ`, 弱軸 `sZ`）。
fn steel_h_props(height: f64, width: f64, web_thick: f64, flange_thick: f64) -> (f64, f64, f64) {
    let shape = SectionShape::SteelBuiltH {
        height,
        upper_width: width,
        lower_width: width,
        upper_thick: flange_thick,
        lower_thick: flange_thick,
        web_thick,
    };
    let a = shape.calc_area();
    let iy = shape.calc_iy();
    let iz = shape.calc_iz();
    let sz_strong = if height > 0.0 { iy * 2.0 / height } else { 0.0 };
    let sz_weak = if width > 0.0 { iz * 2.0 / width } else { 0.0 };
    (a, sz_strong, sz_weak)
}

/// 実配筋梁（[`RcBeamRebar`]）の指定引張側における、RC 部分 1 軸分の断面諸元。
///
/// `tension_is_top` は上端側を引張とするか。応力中心間距離は `j = 7d/8`。
fn beam_axis_props(b: f64, d_full: f64, rebar: &RcBeamRebar, tension_is_top: bool) -> SrcAxisProps {
    let bending = rebar.bending_steel(d_full, tension_is_top);
    let tension = bending.tension;
    SrcAxisProps {
        b,
        d_full,
        dt: tension.centroid_from_edge_mm,
        d: tension.effective_depth_mm,
        at: tension.area_mm2,
        ac: bending.compression.area_mm2,
        j: 7.0 * tension.effective_depth_mm / 8.0,
        pw: rebar.pw(b),
    }
}

/// 実配筋矩形柱（[`RcRectColumnRebar`]）の強軸/弱軸における、RC 部分 1 軸分の断面諸元。
///
/// 強軸は上下辺、弱軸は左右辺の最外段 1 列を引張側とする。応力中心間距離は `j = 7d/8`。
fn column_axis_props(b: f64, d_full: f64, rebar: &RcRectColumnRebar, strong: bool) -> SrcAxisProps {
    let (b_dir, d_dir, edge, aw) = if strong {
        (b, d_full, RectEdge::Top, rebar.aw_x_mm2())
    } else {
        (d_full, b, RectEdge::Left, rebar.aw_y_mm2())
    };
    let steel = rebar.edge_steel(edge, b, d_full);
    let pw = if rebar.hoop.pitch > 0.0 {
        aw / (b_dir * rebar.hoop.pitch)
    } else {
        0.0
    };
    SrcAxisProps {
        b: b_dir,
        d_full: d_dir,
        dt: steel.centroid_from_edge_mm,
        d: steel.effective_depth_mm,
        at: steel.area_mm2,
        ac: steel.area_mm2,
        j: 7.0 * steel.effective_depth_mm / 8.0,
        pw,
    }
}

/// 実配筋梁の曲げ引張側。`mz>0` は下端引張、`mz<0` は上端引張とし、
/// `mz=0` は上下の引張鉄筋量が小さい側を引張とする。
fn beam_tension_is_top(forces: &MemberForcesAt, d_full: f64, rebar: &RcBeamRebar) -> bool {
    if forces.mz < 0.0 {
        true
    } else if forces.mz > 0.0 {
        false
    } else {
        rebar.bending_steel(d_full, true).tension.area_mm2
            < rebar.bending_steel(d_full, false).tension.area_mm2
    }
}

struct SrcShearResult {
    ratio: f64,
    s_q: f64,
    r_q: f64,
    s_qa: f64,
    r_qa: f64,
    alpha: f64,
    pw: f64,
    used_qd: bool,
}

struct SrcSeismicCtx<'a> {
    ctx: &'a DesignCtx,
    pos: f64,
    s_ft_short: f64,
    r_mu: f64,
}

/// SRC 梁の地震時短期の設計用せん断力（構造規定方式、SRC 規準 1987）。
/// `seismic.ctx.seismic_qd` が None、または長期内力に
/// 同一評価位置が見つからない場合は None を返す（呼び出し側は弾性
/// 分担にフォールバックする）。
///
/// - `sQD = sQL + (sM1+sM2)/l′`（`sQL = share・|QL|`、`sM1+sM2 = 2・sZ・sft`）
/// - `rQD1 = rQL + (rMu1+rMu2)/l′`（`rQL = |QL| − sQL`、`rMu1+rMu2 = 2・rMu`）
/// - `rQD2 = max(0, n・(|Q| − sQD))`
///   （SRC規準1987 の `rQD2 = n・(QL+QE−sQD)` を、`QL+QE` = 当該組合せの
///   全せん断力 `|Q|` と読んだもの。`QE` は水平力分のせん断力増分であり、
///   `QL+QE` はその和として組合せ後の全せん断力に一致するとみなした）
/// - `rQD = min(rQD1, rQD2)`（[`crate::QdMethod::Qd1`]/[`crate::QdMethod::Qd2`]
///   選択時はそれぞれ単独。`Qd1` で rQD1 が無効な場合は rQD2 で代替する）
/// - 戻り値は `(sQD, rQD)`。
fn src_beam_seismic_qd(
    seismic: &SrcSeismicCtx,
    q_signed: f64,
    share: f64,
    sz: f64,
) -> Option<(f64, f64)> {
    let qd = seismic.ctx.seismic_qd.as_ref()?;
    let ql_signed = qd
        .long_at
        .iter()
        .find(|(p, _)| (p - seismic.pos).abs() < 1e-6)
        .map(|(_, f)| f[1])?;

    let ql = ql_signed.abs();
    let q = q_signed.abs();

    let s_ql = share * ql;
    let sum_s_m = 2.0 * sz * seismic.s_ft_short;
    let s_qd = if qd.clear_length > 0.0 {
        s_ql + sum_s_m / qd.clear_length
    } else {
        s_ql
    };

    let r_ql = (ql - s_ql).max(0.0);
    let r_qd1 = if qd.clear_length > 0.0 && seismic.r_mu > 0.0 {
        let sum_r_mu = 2.0 * seismic.r_mu;
        r_ql + sum_r_mu / qd.clear_length
    } else {
        f64::INFINITY
    };
    let r_qd2 = (qd.n_factor * (q - s_qd)).max(0.0);

    let r_qd = qd.method.resolve(r_qd1, r_qd2);

    Some((s_qd, r_qd))
}

/// せん断検定の部材モード（SRC規準 1987 の梁・柱の式の切替）。
enum SrcShearMode<'a> {
    /// 梁: 鉄骨・RC の分担検定（長短共通）。
    /// `rQA1 = b·rj·(rα·fs + 0.5·pw·wft)`、`rQA2 = b·rj·(2(b′/b)·fs + pw·wft)`。
    Beam(&'a SrcSeismicCtx<'a>),
    /// 柱（SRC規準 P.96-97）:
    /// - 長期は併用式 `QA = (1+β)·b·rj·a′·fs` を全せん断力と比較する
    ///   （`a′ = rα`（`b′/b ≥ rα/3` のとき）または `3b′/b`、`1 ≤ rα ≤ 2`）。
    /// - 短期は鉄骨部 `sQA`（強軸 `dw·tw·sfs`／弱軸 `(4/3)·bf·tf·sfs`）と
    ///   RC 部 `rQAS1 = b·rj·(fs + 0.5·pw·wft)`（**α を含まない**）・
    ///   `rQAS2 = b·rj·(2(b′/b)·fs + pw·wft)` を分担 `sQD`/`rQD` と比較する。
    ///
    /// `beta` は鉄骨ウェブの形式と寸法による係数
    /// （充腹 `β = n·tw·sd/(b·rj)`、H 形弱軸 `β = 1.33·n·bf·tf/(b·rj)`）。
    Column {
        beta: f64,
        seismic_demand_n: Option<(f64, f64)>,
    },
}

/// 全せん断力を鉄骨部分・RC 部分に分担させ、それぞれの許容せん断力と比較する
/// （梁・柱の両方向で共通利用。式の切替は [`SrcShearMode`]）。
#[allow(clippy::too_many_arguments)]
fn src_shear_check(
    q_signed: f64,
    m_for_alpha: f64,
    q_for_alpha: f64,
    sz: f64,
    at: f64,
    rj: f64,
    rd: f64,
    b: f64,
    b_prime: f64,
    pw_raw: f64,
    fs: f64,
    w_ft: f64,
    s_fs: f64,
    steel_shear_area: f64,
    alpha_max: f64,
    mode: SrcShearMode<'_>,
    term: LoadTerm,
) -> SrcShearResult {
    let alpha = if term == LoadTerm::Short && matches!(mode, SrcShearMode::Column { .. }) {
        1.0
    } else {
        shear_alpha(m_for_alpha, q_for_alpha, rd, alpha_max)
    };
    let q = q_signed.abs();

    let pw_cap = 0.006;
    let pw = pw_raw.min(pw_cap);

    let b_ratio = if b > 1e-9 {
        (b_prime / b).max(0.0)
    } else {
        0.0
    };

    if let SrcShearMode::Column { beta, .. } = mode {
        if term == LoadTerm::Long {
            let a_prime = if b_ratio >= alpha / 3.0 {
                alpha
            } else {
                3.0 * b_ratio
            };
            let qa = (1.0 + beta) * b * rj * a_prime * fs;
            let ratio = if qa > 1e-9 { q / qa } else { 0.0 };
            return SrcShearResult {
                ratio,
                s_q: 0.0,
                r_q: q,
                s_qa: steel_shear_area * s_fs,
                r_qa: qa,
                alpha,
                pw,
                used_qd: false,
            };
        }
    }

    let denom = sz + at * rj;
    let share = if denom > 1e-12 { sz / denom } else { 1.0 };

    let seismic_demand_n = match mode {
        SrcShearMode::Beam(seismic) => src_beam_seismic_qd(seismic, q_signed, share, sz),
        SrcShearMode::Column {
            seismic_demand_n, ..
        } => seismic_demand_n,
    };
    let (s_q, r_q, used_qd) = match seismic_demand_n {
        Some((s_qd, r_qd)) => (s_qd, r_qd, true),
        None => {
            let s_q = share * q;
            (s_q, (q - s_q).max(0.0), false)
        }
    };

    let s_qa = steel_shear_area * s_fs;

    let r_qa1 = match mode {
        SrcShearMode::Beam(_) => b * rj * (alpha * fs + 0.5 * pw * w_ft),
        SrcShearMode::Column { .. } => b * rj * (fs + 0.5 * pw * w_ft),
    };
    let r_qa2 = b * rj * (2.0 * b_ratio * fs + pw * w_ft);
    let r_qa = r_qa1.min(r_qa2);

    let ratio_s = if s_qa > 1e-9 { s_q / s_qa } else { 0.0 };
    let ratio_r = if r_qa > 1e-9 { r_q / r_qa } else { 0.0 };

    SrcShearResult {
        ratio: ratio_s.max(ratio_r),
        s_q,
        r_q,
        s_qa,
        r_qa,
        alpha,
        pw,
        used_qd,
    }
}

/// SRC 梁・SRC 柱の断面検定
/// （`SectionShape::SrcBeamRect`・`SrcColumnRect` を対象とする）。
/// 内蔵 H 形鋼の材種・実板厚や短期地震時の柱せん断が適用条件を満たさない場合、理由付き未算定とする。
pub struct SrcDesign;

impl DesignCheck for SrcDesign {
    fn check(
        &self,
        forces: &MemberForcesAt,
        sec: &Section,
        mat: &Material,
        ctx: &DesignCtx,
    ) -> CheckOutcome {
        let (outcome, incomplete) = self.check_with_incomplete(forces, sec, mat, ctx, false);
        match incomplete {
            Some(reason) => CheckOutcome::Skipped { reason },
            None => outcome,
        }
    }
}

impl SrcDesign {
    /// 検定位置の結果を返す。短期地震時のSRC柱が未算定の場合、理由と、独立に確定した不適合を別の結果で返す。
    /// 未算定を伴う位置で、適合を示す検定比は返さない。
    pub fn check_outcomes(
        &self,
        forces: &MemberForcesAt,
        sec: &Section,
        mat: &Material,
        ctx: &DesignCtx,
    ) -> impl Iterator<Item = CheckOutcome> {
        let (outcome, incomplete) = self.check_with_incomplete(forces, sec, mat, ctx, false);
        crate::member_design_check::strength_outcomes(outcome, incomplete.as_deref())
    }

    pub(crate) fn check_with_incomplete(
        &self,
        forces: &MemberForcesAt,
        sec: &Section,
        mat: &Material,
        ctx: &DesignCtx,
        missing_gravity: bool,
    ) -> (CheckOutcome, Option<String>) {
        match Self::try_check(forces, sec, mat, ctx, missing_gravity) {
            Ok((result, incomplete)) => (CheckOutcome::Checked(result), incomplete),
            Err(reason) => (CheckOutcome::Skipped { reason }, None),
        }
    }

    fn try_check(
        forces: &MemberForcesAt,
        sec: &Section,
        mat: &Material,
        ctx: &DesignCtx,
        missing_gravity: bool,
    ) -> Result<(crate::CheckResult, Option<String>), String> {
        let fc_raw = mat.fc.unwrap_or(0.0);
        if fc_raw <= 0.0 {
            return Err("SRC検定: Fc未設定（Material.fc が None/0 です）".to_string());
        }

        let (shape, member, web_thick_mm, flange_thick_mm) = match &sec.shape {
            Some(shape @ SectionShape::SrcBeamRect { steel_web_thick, steel_flange_thick, .. }) => {
                (shape, "SRC梁", *steel_web_thick, *steel_flange_thick)
            }
            Some(shape @ SectionShape::SrcColumnRect { steel_web_thick, steel_flange_thick, .. }) => {
                (shape, "SRC柱", *steel_web_thick, *steel_flange_thick)
            }
            _ => return Err("SRC検定: 断面形状不一致（Section.shape が SrcBeamRect/SrcColumnRect ではありません）".into()),
        };

        if ctx.rebar_material.is_none() {
            return Err(
                "SRC検定: 主筋の材料が未割当（断面タブで主筋の材料を割り当ててください）"
                    .to_string(),
            );
        }
        if ctx.shear_rebar_material.is_none() {
            return Err("SRC検定: せん断補強筋の材料が未割当\
                         （断面タブでせん断補強筋の材料を割り当ててください）"
                .to_string());
        }
        if let Err(reason) = shape.validate_rebar_size_materials(
            ctx.rebar_material.as_ref(),
            ctx.shear_rebar_material.as_ref(),
        ) {
            return Err(format!("SRC検定: {reason}"));
        }
        if let Some(msg) = sepika_core::material_grade::shear_rebar_material_issue(
            ctx.shear_rebar_material.as_ref(),
        ) {
            return Err(format!("SRC検定: {msg}"));
        }
        let Some(steel_mat) = ctx.steel_material.as_ref() else {
            return Err("SRC検定: 内蔵鉄骨の材料が未割当\
                         （断面タブで内蔵鉄骨の材料を割り当ててください）"
                .to_string());
        };
        let f_value = crate::material_strength::section_steel_f(sec, Some(steel_mat))
            .map_err(|reason| format!("{member}検定: {reason}"))?;
        crate::material_strength::src_h_steel_minimum_thickness_mm(steel_mat)
            .and_then(|minimum_mm| {
                crate::material_strength::validate_src_h_steel_thickness(
                    &steel_mat.name,
                    web_thick_mm,
                    flange_thick_mm,
                    minimum_mm,
                )
            })
            .map_err(|reason| format!("{member}検定: 断面「{}」の{reason}", sec.name))?;

        match shape {
            SectionShape::SrcBeamRect {
                b,
                d,
                rebar,
                steel_height,
                steel_width,
                steel_web_thick,
                steel_flange_thick,
            } => {
                if ctx.kind == MemberKind::Column {
                    return Err(
                        "SRC検定: 梁用断面を柱部材に割り当てています（用途不一致）".to_string()
                    );
                }
                if let Some(qd) = &ctx.seismic_qd {
                    if !forces.pos.is_finite()
                        || !qd
                            .long_at
                            .iter()
                            .find(|(pos, _)| (pos - forces.pos).abs() < 1e-6)
                            .is_some_and(|(_, values)| values[1].is_finite())
                    {
                        return Err("SRC検定: 設計用せん断力に必要な同位置の重力参照せん断力QLが不足または非有限です".into());
                    }
                }
                if rebar.is_unset() {
                    return Err("SRC検定: 配筋が未入力です".to_string());
                }
                if let Err(e) = rebar.validate(*b, *d) {
                    return Err(format!("SRC検定: 配筋が不整合です（{e}）"));
                }
                let tension_is_top = beam_tension_is_top(forces, *d, rebar);
                let props = beam_axis_props(*b, *d, rebar, tension_is_top);
                let props_other = beam_axis_props(*b, *d, rebar, !tension_is_top);
                Ok((
                    beam::src_beam_check(
                        forces,
                        mat,
                        ctx,
                        props,
                        props_other,
                        rebar.main_dia.designation_or_diameter_mm(),
                        *steel_height,
                        *steel_width,
                        *steel_web_thick,
                        *steel_flange_thick,
                        f_value,
                        fc_raw,
                    ),
                    None,
                ))
            }
            SectionShape::SrcColumnRect {
                b,
                d,
                rebar,
                steel_height,
                steel_width,
                steel_web_thick,
                steel_flange_thick,
            } => {
                if matches!(ctx.kind, MemberKind::Girder | MemberKind::Brace) {
                    return Err(
                        "SRC検定: 柱用断面を梁部材に割り当てています（用途不一致）".to_string()
                    );
                }
                if ctx.term == LoadTerm::Short && (missing_gravity || ctx.seismic_qd.is_some()) {
                    if !forces.n.is_finite() {
                        return Err("SRC柱検定: 組合せの軸力Nが非有限です".into());
                    }
                    if ![
                        fc_raw,
                        *b,
                        *d,
                        *steel_height,
                        *steel_width,
                        *steel_web_thick,
                        *steel_flange_thick,
                    ]
                    .iter()
                    .all(|v| v.is_finite() && *v > 0.0)
                    {
                        return Err(
                            "SRC柱検定: Fcと断面・内蔵鉄骨の寸法には有限な正値が必要です".into(),
                        );
                    }
                }
                if rebar.is_unset() {
                    return Err("SRC検定: 配筋が未入力です".to_string());
                }
                if let Err(e) = rebar.validate(*b, *d) {
                    return Err(format!("SRC検定: 配筋が不整合です（{e}）"));
                }
                let props_z = column_axis_props(*b, *d, rebar, true);
                let props_y = column_axis_props(*b, *d, rebar, false);
                let ag = rebar.total_main_area();
                let at_perp_z = (ag - props_z.at - props_z.ac).max(0.0);
                let at_perp_y = (ag - props_y.at - props_y.ac).max(0.0);
                Ok(column::src_column_check(
                    forces,
                    mat,
                    ctx,
                    props_z,
                    props_y,
                    at_perp_z,
                    at_perp_y,
                    ag,
                    rebar.main_dia.designation_or_diameter_mm(),
                    rebar.main_dia.designation_or_diameter_mm(),
                    *steel_height,
                    *steel_width,
                    *steel_web_thick,
                    *steel_flange_thick,
                    f_value,
                    fc_raw,
                    missing_gravity,
                ))
            }
            _ => unreachable!(),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
