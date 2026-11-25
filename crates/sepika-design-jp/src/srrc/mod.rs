//! SRC 梁・柱と柱梁接合部の許容応力度検定。
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
pub mod joint_hoops;
pub mod panel_zone;

pub(crate) use crate::ratio_or_large;
pub(crate) use crate::rc::{shear_alpha, AxisProps as SrcAxisProps};
pub use column::{
    SrcColumnHeadLimit, SrcColumnRoute21RcShear, SrcColumnRoute21Shear, SrcColumnRoute21ShearAxis,
    SrcColumnSeismicBending, SrcColumnSeismicBendingAxis, SrcColumnSeismicBendingEnd,
    SrcColumnSeismicSense,
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
    s_q: Option<f64>,
    r_q: Option<f64>,
    s_qa: f64,
    r_qa: Option<f64>,
}

#[derive(Clone, Copy)]
struct SrcShortShearDemand {
    steel_n: Option<f64>,
    rc_n: Option<f64>,
}

#[allow(clippy::too_many_arguments)]
fn src_column_shear_check(
    q_signed: f64,
    m_for_alpha: f64,
    q_for_alpha: f64,
    rj: f64,
    rd: f64,
    b: f64,
    b_prime: f64,
    pw_raw: f64,
    fs: f64,
    w_ft: Option<f64>,
    s_fs: f64,
    steel_shear_area: f64,
    alpha_max: f64,
    beta: f64,
    short_demand_n: Option<SrcShortShearDemand>,
    term: LoadTerm,
) -> Option<SrcShearResult> {
    let alpha = if term == LoadTerm::Short {
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

    if term == LoadTerm::Long {
        let a_prime = alpha.min(3.0 * b_ratio);
        let qa = (1.0 + beta) * b * rj * a_prime * fs;
        if !q.is_finite() || !qa.is_finite() {
            return None;
        }
        let ratio = ratio_or_large(q, qa);
        return Some(SrcShearResult {
            ratio,
            s_q: None,
            r_q: Some(q),
            s_qa: steel_shear_area * s_fs,
            r_qa: Some(qa),
        });
    }

    let SrcShortShearDemand {
        steel_n: s_q,
        rc_n: r_q,
    } = short_demand_n?;
    let s_qa = steel_shear_area * s_fs;

    let pw_term = if pw == 0.0 {
        Some(0.0)
    } else {
        w_ft.map(|w_ft| pw * w_ft)
    };
    let r_qa = pw_term.map(|pw_term| {
        let r_qa1 = b * rj * (fs + 0.5 * pw_term);
        let r_qa2 = b * rj * (2.0 * b_ratio * fs + pw_term);
        r_qa1.min(r_qa2)
    });

    let ratio_s = s_q.filter(|_| s_qa > 1e-9).map_or(0.0, |q| q / s_qa);
    let ratio_r = r_q
        .zip(r_qa)
        .filter(|(_, qa)| *qa > 1e-9)
        .map_or(0.0, |(q, qa)| q / qa);

    Some(SrcShearResult {
        ratio: ratio_s.max(ratio_r),
        s_q,
        r_q,
        s_qa,
        r_qa,
    })
}

const MISSING_MAIN_REBAR: &str = "主筋の材料が未割当（断面タブで主筋の材料を割り当ててください）";

fn uses_explicit_route21_column(sec: &Section, ctx: &DesignCtx) -> bool {
    ctx.term == LoadTerm::Short
        && ctx.kind == MemberKind::Column
        && ctx.src_column_route21_shear.is_some()
        && matches!(sec.shape, Some(SectionShape::SrcColumnRect { .. }))
}

pub(crate) fn preserve_route21_shear_detail(
    sec: &Section,
    ctx: &DesignCtx,
    outcome: &CheckOutcome,
    incomplete: &mut Option<String>,
) {
    if !uses_explicit_route21_column(sec, ctx) {
        return;
    }
    let (Some(reason), CheckOutcome::Checked(result)) = (incomplete, outcome) else {
        return;
    };
    if let Some(shear) = result
        .components
        .iter()
        .find(|component| component.kind == crate::CheckKind::Shear && component.ratio <= 1.0)
    {
        reason.push_str(&format!("。せん断の確認結果（{}）", shear.detail));
    }
}

/// 矩形SRC梁・柱の断面検定（`SectionShape::SrcBeamRect`・`SrcColumnRect` を対象とする）。
/// 断面形状・配筋、内蔵 H 形鋼の材種・実板厚、短期地震時の柱せん断が適用条件を満たさない場合は理由付き未算定とする。
/// SRC柱で軸力が非有限の位置は理由付き未算定とする。
pub struct SrcDesign;

impl DesignCheck for SrcDesign {
    fn check(
        &self,
        forces: &MemberForcesAt,
        sec: &Section,
        mat: &Material,
        ctx: &DesignCtx,
    ) -> CheckOutcome {
        let (outcome, mut incomplete) = self.check_with_incomplete(forces, sec, mat, ctx, false);
        preserve_route21_shear_detail(sec, ctx, &outcome, &mut incomplete);
        match incomplete {
            Some(reason) => CheckOutcome::Skipped { reason },
            None => outcome,
        }
    }
}

impl SrcDesign {
    /// 鉄筋の材料区分、短期地震時のSRC柱の適用条件や材軸ねじりで未算定となる場合、理由と独立に確定した不適合を別の結果で返す。
    /// 未算定を伴う位置で、適合を示す検定比は返さない。
    /// 地震時の重力参照内力が必要で、対象部材の参照内力がない場合は `missing_gravity` に true を渡す。
    pub fn check_outcomes(
        &self,
        forces: &MemberForcesAt,
        sec: &Section,
        mat: &Material,
        ctx: &DesignCtx,
        missing_gravity: bool,
    ) -> impl Iterator<Item = CheckOutcome> {
        self.check_outcomes_with_shear_context(forces, sec, mat, ctx, missing_gravity, None)
    }

    /// 時刻歴の地震内力に対する短期検定。設計用せん断力の参照がない場合は、通常短期せん断を適用しない。
    pub fn check_time_history_outcomes(
        &self,
        forces: &MemberForcesAt,
        sec: &Section,
        mat: &Material,
        ctx: &DesignCtx,
        missing_gravity: bool,
    ) -> impl Iterator<Item = CheckOutcome> {
        let ordinary_shear_issue = (ctx.term == LoadTerm::Short
            && !missing_gravity
            && ctx.seismic_qd.is_none()
            && (forces.qy != 0.0 || forces.qz != 0.0))
            .then_some("時刻歴の地震内力から設計用せん断力と材料別の負担曲げを確定できません");
        self.check_outcomes_with_shear_context(
            forces,
            sec,
            mat,
            ctx,
            missing_gravity,
            ordinary_shear_issue,
        )
    }

    fn check_outcomes_with_shear_context(
        &self,
        forces: &MemberForcesAt,
        sec: &Section,
        mat: &Material,
        ctx: &DesignCtx,
        missing_gravity: bool,
        ordinary_shear_issue: Option<&str>,
    ) -> impl Iterator<Item = CheckOutcome> {
        use crate::member_design_check::{
            combine_incomplete_reasons, concrete_member_torsion_reason, strength_outcomes,
        };
        let (outcome, mut incomplete) = self.check_with_shear_context(
            forces,
            sec,
            mat,
            ctx,
            missing_gravity,
            ordinary_shear_issue,
        );
        if matches!(outcome, CheckOutcome::Skipped { .. })
            && ctx.term == LoadTerm::Short
            && ctx.src_column_route21_shear.is_none()
            && ctx.kind == MemberKind::Column
            && matches!(sec.shape, Some(SectionShape::SrcColumnRect { .. }))
        {
            incomplete = combine_incomplete_reasons(
                incomplete,
                column::gravity_reference_issue(
                    forces.pos,
                    ctx.seismic_qd.as_ref(),
                    missing_gravity,
                ),
            );
        }
        let torsion =
            concrete_member_torsion_reason(sepika_core::structure_kind::StructureKind::Src, forces);
        let mut reason = combine_incomplete_reasons(incomplete, torsion);
        preserve_route21_shear_detail(sec, ctx, &outcome, &mut reason);
        strength_outcomes(outcome, reason.as_deref())
    }

    pub(crate) fn check_with_incomplete(
        &self,
        forces: &MemberForcesAt,
        sec: &Section,
        mat: &Material,
        ctx: &DesignCtx,
        missing_gravity: bool,
    ) -> (CheckOutcome, Option<String>) {
        self.check_with_shear_context(forces, sec, mat, ctx, missing_gravity, None)
    }

    fn check_with_shear_context(
        &self,
        forces: &MemberForcesAt,
        sec: &Section,
        mat: &Material,
        ctx: &DesignCtx,
        missing_gravity: bool,
        ordinary_shear_issue: Option<&str>,
    ) -> (CheckOutcome, Option<String>) {
        let main_rebar = ctx.rebar_material.as_ref();
        let material_issue = crate::material_strength::main_rebar_category_issue(main_rebar)
            .or_else(|| {
                let grade = main_rebar?.name.trim();
                (!sepika_core::material_grade::is_src_main_rebar_grade(grade)).then(|| {
                    format!("主筋材料「{grade}」はSRC規準の異形主筋に対応する材種ではありません")
                })
            })
            .or_else(|| {
                (main_rebar.is_none() && uses_explicit_route21_column(sec, ctx))
                    .then(|| MISSING_MAIN_REBAR.to_owned())
            })
            .map(|reason| format!("SRC検定: 断面「{}」の{reason}", sec.name));
        let shear_material = ctx.shear_rebar_material.as_ref();
        let uses_shear_rebar = ordinary_shear_issue.is_none()
            && match sec.shape.as_ref() {
                Some(SectionShape::SrcBeamRect { b, rebar, .. }) => {
                    forces.qy != 0.0
                        && rebar.pw(*b) > 0.0
                        && (ctx.term != LoadTerm::Short
                            || (!missing_gravity && ctx.seismic_qd.is_none()))
                }
                Some(SectionShape::SrcColumnRect { rebar, .. }) if ctx.term == LoadTerm::Short => {
                    let seismic = column::is_short_seismic(ctx, missing_gravity);
                    (rebar.aw_x_mm2() > 0.0 && (seismic || forces.qy != 0.0))
                        || (rebar.aw_y_mm2() > 0.0 && (seismic || forces.qz != 0.0))
                }
                _ => false,
            };
        let shear_material_issue = if uses_shear_rebar {
            crate::material_strength::shear_rebar_category_issue(sec, shear_material)
                .or_else(|| {
                    let mat = shear_material?;
                    sepika_core::material_grade::shear_rebar_fy_issue(mat).or_else(|| {
                        sepika_core::material_grade::shear_rebar_material_issue(Some(mat))
                    })
                })
                .or_else(|| {
                    shear_material
                        .is_none()
                        .then(|| "SRC検定: せん断補強筋の材料が未割当です".into())
                })
                .or_else(|| {
                    sec.shape
                        .as_ref()?
                        .validate_rebar_size_materials(None, shear_material)
                        .err()
                })
        } else {
            None
        };
        let main_rebar_valid = material_issue.is_none();
        let shear_rebar_valid = shear_material_issue.is_none();
        let material_issue = crate::member_design_check::combine_incomplete_reasons(
            material_issue,
            shear_material_issue.as_deref(),
        );
        match Self::try_check(
            forces,
            sec,
            mat,
            ctx,
            missing_gravity,
            ordinary_shear_issue,
            main_rebar_valid,
            shear_rebar_valid,
        ) {
            Ok((result, incomplete)) => (
                CheckOutcome::Checked(result),
                crate::member_design_check::combine_incomplete_reasons(
                    material_issue,
                    incomplete.as_deref().or(ordinary_shear_issue),
                ),
            ),
            Err(reason) => (CheckOutcome::Skipped { reason }, material_issue),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn try_check(
        forces: &MemberForcesAt,
        sec: &Section,
        mat: &Material,
        ctx: &DesignCtx,
        missing_gravity: bool,
        ordinary_shear_issue: Option<&str>,
        main_rebar_valid: bool,
        shear_rebar_valid: bool,
    ) -> Result<(crate::CheckResult, Option<String>), String> {
        let fc_raw = mat.fc.unwrap_or(0.0);
        if !fc_raw.is_finite() || fc_raw <= 0.0 {
            return Err("SRC検定: Fc未設定（Material.fc が None/0 です）".to_string());
        }
        if fc_raw > 60.0 {
            return Err("SRC検定: コンクリートの設計基準強度が60 N/mm²を超えています".into());
        }
        if ctx
            .steel_attr
            .as_ref()
            .is_some_and(sepika_core::model::SteelDesignAttr::has_section_loss)
        {
            return Err("SRC検定: 欠損を含む内蔵鉄骨の有効断面を評価できません".into());
        }

        let (shape, member, height_mm, width_mm, web_thick_mm, flange_thick_mm, main_size) = match &sec.shape {
            Some(shape @ SectionShape::SrcBeamRect { rebar, steel_height, steel_width, steel_web_thick, steel_flange_thick, .. }) => {
                (shape, "SRC梁", *steel_height, *steel_width, *steel_web_thick, *steel_flange_thick, rebar.main_dia)
            }
            Some(shape @ SectionShape::SrcColumnRect { rebar, steel_height, steel_width, steel_web_thick, steel_flange_thick, .. }) => {
                (shape, "SRC柱", *steel_height, *steel_width, *steel_web_thick, *steel_flange_thick, rebar.main_dia)
            }
            _ => return Err("SRC検定: 断面形状不一致（Section.shape が SrcBeamRect/SrcColumnRect ではありません）".into()),
        };
        if main_rebar_valid && !(13.0..=41.0).contains(&main_size.designation_or_diameter_mm()) {
            return Err(format!("{member}検定: 主筋はD13〜D41の異形鉄筋が対象です"));
        }

        if ctx.rebar_material.is_none() && !uses_explicit_route21_column(sec, ctx) {
            return Err(format!("SRC検定: {MISSING_MAIN_REBAR}"));
        }
        if let Err(reason) = shape.validate_rebar_size_materials(
            ctx.rebar_material.as_ref().filter(|_| main_rebar_valid),
            None,
        ) {
            return Err(format!("SRC検定: {reason}"));
        }
        let Some(steel_mat) = ctx.steel_material.as_ref() else {
            return Err("SRC検定: 内蔵鉄骨の材料が未割当\
                         （断面タブで内蔵鉄骨の材料を割り当ててください）"
                .to_string());
        };
        let resolve_f = || {
            crate::material_strength::section_steel_f(sec, Some(steel_mat))
                .map_err(|reason| format!("{member}検定: 内蔵鉄骨「{}」: {reason}", steel_mat.name))
        };
        let minimum_mm = match crate::material_strength::src_h_steel_minimum_thickness_mm(steel_mat)
        {
            Ok(minimum_mm) => minimum_mm,
            Err(reason) => {
                // 表にない名称では直接入力した降伏応力の不備も診断する。
                if sepika_core::material_grade::steel_grade_base(&steel_mat.name).is_none() {
                    resolve_f()?;
                }
                return Err(format!("{member}検定: 断面「{}」の{reason}", sec.name));
            }
        };
        let f_value = resolve_f()?;
        crate::material_strength::validate_src_h_steel_thickness(
            &steel_mat.name,
            web_thick_mm,
            flange_thick_mm,
            minimum_mm,
        )
        .map_err(|reason| format!("{member}検定: 断面「{}」の{reason}", sec.name))?;
        shape.validate_src_geometry().map_err(|reason| {
            format!(
                "{member}検定: 断面「{}」の形状・配筋が不整合です（{reason}）",
                sec.name
            )
        })?;
        let flange_ratio = width_mm / (2.0 * flange_thick_mm);
        let web_ratio = (height_mm - 2.0 * flange_thick_mm) / web_thick_mm;
        let flange_f =
            sepika_core::material_grade::resolve_steel_f(Some(steel_mat), Some(flange_thick_mm))?;
        let web_f =
            sepika_core::material_grade::resolve_steel_f(Some(steel_mat), Some(web_thick_mm))?;
        sepika_core::material_grade::validate_src_h_width_thickness(
            flange_ratio,
            web_ratio,
            flange_f,
            web_f,
            matches!(shape, SectionShape::SrcColumnRect { .. }),
        )
        .map_err(|reason| format!("{member}検定: {reason}"))?;

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
                if rebar.is_unset() {
                    return Err("SRC検定: 配筋が未入力です".to_string());
                }
                let tension_is_top = beam_tension_is_top(forces, *d, rebar);
                let props = beam_axis_props(*b, *d, rebar, tension_is_top);
                let props_other = beam_axis_props(*b, *d, rebar, !tension_is_top);
                Ok(beam::src_beam_check(
                    forces,
                    ctx,
                    mat,
                    props,
                    props_other.dt,
                    rebar.main_dia.designation_or_diameter_mm(),
                    *steel_height,
                    *steel_width,
                    *steel_web_thick,
                    *steel_flange_thick,
                    f_value,
                    ctx.term == LoadTerm::Short && (missing_gravity || ctx.seismic_qd.is_some()),
                    ordinary_shear_issue,
                    main_rebar_valid,
                    shear_rebar_valid,
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
                if column::is_short_seismic(ctx, missing_gravity) {
                    if !forces.n.is_finite() && ctx.src_column_route21_shear.is_none() {
                        return Err("SRC柱検定: 組合せの軸力Nが非有限です".into());
                    }
                    if !fc_raw.is_finite() {
                        return Err("SRC柱検定: Fcには有限な正値が必要です".into());
                    }
                }
                if rebar.is_unset() {
                    return Err("SRC検定: 配筋が未入力です".to_string());
                }
                if !forces.n.is_finite()
                    && (ctx.term != LoadTerm::Short || ctx.src_column_route21_shear.is_none())
                {
                    return Err("SRC柱検定: 設計軸力Nが非有限です".into());
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
                    ordinary_shear_issue,
                    main_rebar_valid,
                    shear_rebar_valid,
                ))
            }
            _ => unreachable!(),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
