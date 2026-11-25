//! RC 造の許容応力度と断面検定（RC 規準13〜18条・令82条による許容応力度計算）。

use crate::member_design_check::combine_incomplete_reasons;
use crate::member_design_check::rc_column_long_term_shear::LongTermColumnShear;
use crate::{CheckOutcome, DesignCheck, DesignCtx, MemberForcesAt, MemberKind};
use sepika_core::model::{Material, Section};
use sepika_core::section_shape::SectionShape;

mod beam;
mod bond;
mod clear_span;
mod column;
/// RC 柱の短期設計せん断力用 ΣMy（崩壊メカニズム判定）。
pub mod column_mechanism;
pub mod joint;
mod provisions;
pub mod wall;
/// 鉄筋コンクリート造耐震壁のせん断非線形特性（トリリニア Qc/βu/Qu）。
pub mod wall_nonlinear;

mod allowable;
mod design_shear;
pub(crate) mod section_props;
mod shear_capacity;

pub use beam::beam_qd1_clear_span;
pub(crate) use beam::{
    beam_supports_slab, column_transmits_bending, frame_end_positions, restrains_direction,
    transmits_bending,
};
pub use bond::{rc_beam_bond_check_1991, Bond1991Result, BondCheckResult};
pub(crate) use bond::{rc_beam_bond_check_1991_with_rebar, rc_beam_bond_check_with_rebar};
pub use clear_span::{rc_member_clear_span, RcClearSpanError};
pub use column_mechanism::{
    compute_column_mechanism_sum_my, design_axial_for_mechanism,
    time_history_column_mechanism_sum_my, ColumnMechanism, ColumnMechanismTopology,
};
pub use wall_nonlinear::{
    wall_shear_beta_s, wall_shear_crack, wall_shear_trilinear, wall_shear_ultimate,
    WallShearTrilinear, WallShearTrilinearInput,
};

pub(crate) use crate::material_strength::concrete_allowable_bond_for_rebar;
pub use crate::material_strength::{
    concrete_allowable_bond, concrete_allowable_compression, concrete_allowable_shear,
    concrete_allowable_shear_class, concrete_young_modulus, main_rebar_grade,
    rebar_allowable_shear, rebar_allowable_tension, rebar_sigma_y_of, shear_rebar_grade,
    young_ratio_n,
};

pub(crate) use allowable::*;
pub(crate) use column::interp_ma;
use column::long_term_shear::LongTermColumnShearCheck;
pub(crate) use column::long_term_shear::{recovered_long_shear_detail, shear_check_result};
pub(crate) use design_shear::*;
pub(crate) use section_props::*;
pub(crate) use shear_capacity::*;

pub struct RcDesign;

#[derive(Clone, Copy)]
pub(crate) enum ColumnGravityMode {
    SingleOutcome,
    Ordinary {
        boundary_shear_issue: Option<&'static str>,
        response_end_axial_issue: Option<&'static str>,
        saved_long_term: bool,
    },
    TimeHistory {
        strict_gravity: bool,
    },
}

impl DesignCheck for RcDesign {
    fn check(
        &self,
        forces: &MemberForcesAt,
        sec: &Section,
        mat: &Material,
        ctx: &DesignCtx,
    ) -> CheckOutcome {
        let (outcome, incomplete) = self.check_with_gravity_mode(
            forces,
            sec,
            mat,
            ctx,
            None,
            ColumnGravityMode::SingleOutcome,
        );
        match incomplete {
            Some(reason) => CheckOutcome::Skipped { reason },
            None => outcome,
        }
    }
}

impl RcDesign {
    /// 時刻歴の簡易検定で未算定理由がある位置では、独立に確定した不適合を別に返す。
    /// 時刻歴のRC柱で地震時せん断を照査する場合は、参照欠損時も `ctx.seismic_qd` を設定する。
    /// その他の部材で必要な重力参照内力を用意できない場合は `missing_gravity` に true を渡す。
    pub fn check_outcomes(
        &self,
        forces: &MemberForcesAt,
        sec: &Section,
        mat: &Material,
        ctx: &DesignCtx,
        missing_gravity: bool,
    ) -> impl Iterator<Item = CheckOutcome> {
        use crate::member_design_check::{
            combine_incomplete_reasons, concrete_member_torsion_reason, strength_outcomes,
        };
        let strict_column_gravity = ctx.term == crate::LoadTerm::Short
            && ctx.seismic_qd.is_some()
            && matches!(
                sec.shape,
                Some(SectionShape::RcColumnRect { .. } | SectionShape::RcColumnCircle { .. })
            );
        let missing_shear_reference = (missing_gravity && !strict_column_gravity)
            .then_some("設計用せん断力に必要な重力参照内力が不足しています");
        let (outcome, incomplete) = self.check_with_gravity_mode(
            forces,
            sec,
            mat,
            ctx,
            missing_shear_reference,
            ColumnGravityMode::TimeHistory {
                strict_gravity: strict_column_gravity,
            },
        );
        let torsion =
            concrete_member_torsion_reason(sepika_core::structure_kind::StructureKind::Rc, forces);
        let reason = combine_incomplete_reasons(incomplete, torsion);
        strength_outcomes(outcome, reason.as_deref())
    }

    pub(crate) fn check_ordinary_with_incomplete(
        &self,
        forces: &MemberForcesAt,
        sec: &Section,
        mat: &Material,
        ctx: &DesignCtx,
        missing_shear_reference: Option<&str>,
        column_gravity: ColumnGravityMode,
    ) -> (CheckOutcome, Option<String>) {
        self.check_with_gravity_mode(
            forces,
            sec,
            mat,
            ctx,
            missing_shear_reference,
            column_gravity,
        )
    }

    fn check_with_gravity_mode(
        &self,
        forces: &MemberForcesAt,
        sec: &Section,
        mat: &Material,
        ctx: &DesignCtx,
        missing_shear_reference: Option<&str>,
        column_gravity: ColumnGravityMode,
    ) -> (CheckOutcome, Option<String>) {
        let material_issue =
            crate::material_strength::main_rebar_category_issue(ctx.rebar_material.as_ref())
                .or_else(|| {
                    crate::material_strength::main_rebar_grade_requiring_approval(main_rebar_grade(
                        ctx.rebar_material.as_ref(),
                    ))
                    .map(|family| format!("{family}系主筋の認定に基づく許容応力度を確認できません"))
                })
                .map(|reason| format!("RC 検定: 断面「{}」の{reason}", sec.name));
        let shear_material_issue = crate::material_strength::shear_rebar_category_issue(
            sec,
            ctx.shear_rebar_material.as_ref(),
        );
        let main_rebar_valid = material_issue.is_none();
        let shear_rebar_valid = shear_material_issue.is_none();
        let beam = matches!(sec.shape, Some(SectionShape::RcBeamRect { .. }));
        let shear_issue = missing_shear_reference;
        let mut section_issue = None;
        let outcome = if let (false, true, true, Some(reason)) = (
            beam,
            main_rebar_valid,
            shear_rebar_valid,
            missing_shear_reference,
        ) {
            CheckOutcome::Skipped {
                reason: reason.into(),
            }
        } else {
            Self::check_section(
                forces,
                sec,
                mat,
                ctx,
                main_rebar_valid,
                shear_rebar_valid,
                shear_issue,
                column_gravity,
                &mut section_issue,
            )
        };
        let incomplete =
            combine_incomplete_reasons(material_issue, shear_material_issue.as_deref());
        let incomplete = if beam || incomplete.is_some() {
            combine_incomplete_reasons(incomplete, shear_issue)
        } else {
            incomplete
        };
        let incomplete = combine_incomplete_reasons(incomplete, section_issue.as_deref());
        let strict_column_gravity = matches!(
            column_gravity,
            ColumnGravityMode::TimeHistory {
                strict_gravity: true
            }
        );
        let incomplete = if let Some(qd) = ctx.seismic_qd.as_ref().filter(|_| strict_column_gravity)
        {
            [1, 2].into_iter().fold(incomplete, |reason, index| {
                combine_incomplete_reasons(
                    reason,
                    column_qd2_reference_issue(qd, forces.pos, index),
                )
            })
        } else {
            match sec.shape.as_ref() {
                Some(shape) if !shear_rebar_valid && missing_shear_reference.is_none() => {
                    column::gravity_shear_reference_issues(ctx, forces.pos, shape).fold(
                        incomplete,
                        |reason, reference_issue| {
                            combine_incomplete_reasons(reason, Some(reference_issue))
                        },
                    )
                }
                _ => incomplete,
            }
        };
        let incomplete = if let Some(mechanism) = column::qd1_mechanism(ctx) {
            [&mechanism.qy, &mechanism.qz]
                .into_iter()
                .filter_map(|direction| direction.as_ref().err())
                .fold(incomplete, |reason, axis_reason| {
                    combine_incomplete_reasons(reason, Some(axis_reason))
                })
        } else {
            incomplete
        };
        (outcome, incomplete)
    }

    #[allow(clippy::too_many_arguments)]
    fn check_section(
        forces: &MemberForcesAt,
        sec: &Section,
        mat: &Material,
        ctx: &DesignCtx,
        main_rebar_valid: bool,
        shear_rebar_valid: bool,
        shear_issue: Option<&str>,
        column_gravity: ColumnGravityMode,
        section_issue: &mut Option<String>,
    ) -> CheckOutcome {
        let (shape, fc_raw) =
            match Self::validate_section(sec, mat, ctx, main_rebar_valid, shear_rebar_valid) {
                Ok(input) => input,
                Err(reason) => return CheckOutcome::Skipped { reason },
            };
        if let (false, true, true, Some(reason)) = (
            matches!(shape, SectionShape::RcBeamRect { .. }),
            main_rebar_valid,
            shear_rebar_valid,
            shear_issue,
        ) {
            return CheckOutcome::Skipped {
                reason: reason.into(),
            };
        }

        match shape {
            SectionShape::RcBeamRect { rebar, .. } => beam::beam_check(
                forces,
                mat,
                ctx,
                shape,
                rebar,
                fc_raw,
                main_rebar_valid,
                shear_rebar_valid,
                shear_issue.is_none(),
                section_issue,
            ),
            _ => column::column_check(
                forces,
                mat,
                ctx,
                shape,
                fc_raw,
                main_rebar_valid,
                shear_rebar_valid,
                shear_issue.is_none(),
                column_gravity,
                section_issue,
            ),
        }
    }

    pub(crate) fn long_column_shear_check(
        sec: &Section,
        mat: &Material,
        ctx: &DesignCtx,
        envelope: &LongTermColumnShear,
    ) -> Result<LongTermColumnShearCheck, String> {
        let main_rebar_valid =
            crate::material_strength::main_rebar_category_issue(ctx.rebar_material.as_ref())
                .is_none();
        let shear_rebar_valid = crate::material_strength::shear_rebar_category_issue(
            sec,
            ctx.shear_rebar_material.as_ref(),
        )
        .is_none();
        let (shape, fc_raw) =
            Self::validate_section(sec, mat, ctx, main_rebar_valid, shear_rebar_valid)?;
        if !fc_raw.is_finite() {
            return Err("RC 検定: Fc が非有限です".into());
        }
        let allow = rc_allow(fc_raw, mat.concrete_class, None, true);
        Ok(column::long_term_shear::check(
            shape, &allow, ctx.length, envelope,
        ))
    }

    fn validate_section<'a>(
        sec: &'a Section,
        mat: &Material,
        ctx: &DesignCtx,
        main_rebar_valid: bool,
        shear_rebar_valid: bool,
    ) -> Result<(&'a SectionShape, f64), String> {
        let fc_raw = mat.fc.unwrap_or(0.0);
        if fc_raw <= 0.0 {
            return Err("RC 検定: Fc 未設定（Material.fc が None/0 です。コンクリート強度を設定してください）".into());
        }

        let new_rebar_unset = match &sec.shape {
            Some(SectionShape::RcBeamRect { rebar, .. }) => rebar.is_unset(),
            Some(SectionShape::RcColumnRect { rebar, .. }) => rebar.is_unset(),
            Some(SectionShape::RcColumnCircle { rebar, .. }) => rebar.is_unset(),
            _ => false,
        };
        if new_rebar_unset {
            return Err("RC 検定: 配筋が未入力です".into());
        }

        let shape = match &sec.shape {
            Some(
                s @ (SectionShape::RcBeamRect { .. }
                | SectionShape::RcColumnRect { .. }
                | SectionShape::RcColumnCircle { .. }),
            ) => s,
            _ => {
                return Err("RC 検定: 配筋情報なし\
                             （Section.shape が RcBeamRect/\
                             RcColumnRect/RcColumnCircle ではありません）"
                    .into());
            }
        };

        if ctx.rebar_material.is_none() {
            return Err(
                "RC 検定: 主筋の材料が未割当（断面タブで主筋の材料を割り当ててください）".into(),
            );
        }
        if ctx.shear_rebar_material.is_none() {
            return Err("RC 検定: せん断補強筋の材料が未割当\
                         （断面タブでせん断補強筋の材料を割り当ててください）"
                .into());
        }
        if let Err(reason) = shape.validate_rebar_size_materials(
            ctx.rebar_material.as_ref().filter(|_| main_rebar_valid),
            ctx.shear_rebar_material
                .as_ref()
                .filter(|_| shear_rebar_valid),
        ) {
            return Err(format!("RC 検定: {reason}"));
        }
        if let Some(msg) = sepika_core::material_grade::shear_rebar_material_issue(
            ctx.shear_rebar_material
                .as_ref()
                .filter(|_| shear_rebar_valid),
        ) {
            return Err(format!("RC 検定: {msg}"));
        }

        if matches!(shape, SectionShape::RcBeamRect { .. }) && ctx.kind == MemberKind::Column {
            return Err("RC 検定: 梁用断面を柱部材に割り当てています（用途不一致）".into());
        }
        if matches!(
            shape,
            SectionShape::RcColumnRect { .. } | SectionShape::RcColumnCircle { .. }
        ) && matches!(ctx.kind, MemberKind::Girder | MemberKind::Brace)
        {
            return Err("RC 検定: 柱用断面を梁部材に割り当てています（用途不一致）".into());
        }

        let rebar_issue = match shape {
            SectionShape::RcBeamRect { b, d, rebar } => rebar.validate(*b, *d).err(),
            SectionShape::RcColumnRect { b, d, rebar } => rebar.validate(*b, *d).err(),
            SectionShape::RcColumnCircle { d, rebar } => rebar.validate(*d).err(),
            _ => None,
        };
        if let Some(e) = rebar_issue {
            return Err(format!("RC 検定: 配筋が不整合です（{e}）"));
        }
        Ok((shape, fc_raw))
    }
}

#[cfg(test)]
mod tests;
