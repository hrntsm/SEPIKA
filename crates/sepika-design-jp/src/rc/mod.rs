//! RC 造の許容応力度と断面検定（RC 規準13〜18条・令82条による許容応力度計算）。

use crate::member_design_check::combine_incomplete_reasons;
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

pub use bond::{rc_beam_bond_check_1991, Bond1991Result, BondCheckResult};
pub(crate) use bond::{rc_beam_bond_check_1991_with_rebar, rc_beam_bond_check_with_rebar};
pub use clear_span::{rc_member_clear_span, RcClearSpanError};
pub use column_mechanism::{
    compute_column_mechanism_sum_my, design_axial_for_mechanism, resolve_column_end_hinge,
    sum_my_from_end_hinges, ColumnEndHinge, ColumnMechanism,
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
pub(crate) use design_shear::*;
pub(crate) use section_props::*;
pub(crate) use shear_capacity::*;

pub struct RcDesign;

impl DesignCheck for RcDesign {
    fn check(
        &self,
        forces: &MemberForcesAt,
        sec: &Section,
        mat: &Material,
        ctx: &DesignCtx,
    ) -> CheckOutcome {
        let (outcome, incomplete) = self.check_with_incomplete(forces, sec, mat, ctx, None);
        match incomplete {
            Some(reason) => CheckOutcome::Skipped { reason },
            None => outcome,
        }
    }
}

impl RcDesign {
    /// 鉄筋の材料区分または材軸ねじりで未算定の場合、独立に確定した不適合を別に返す。
    /// 必要な重力参照内力を用意できない場合は `missing_gravity` に true を渡す。
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
        let missing_shear_reference =
            missing_gravity.then_some("設計用せん断力に必要な重力参照内力が不足しています");
        let (outcome, incomplete) =
            self.check_with_incomplete(forces, sec, mat, ctx, missing_shear_reference);
        let torsion =
            concrete_member_torsion_reason(sepika_core::structure_kind::StructureKind::Rc, forces);
        let reason = combine_incomplete_reasons(incomplete, torsion);
        strength_outcomes(outcome, reason.as_deref())
    }

    pub(crate) fn check_with_incomplete(
        &self,
        forces: &MemberForcesAt,
        sec: &Section,
        mat: &Material,
        ctx: &DesignCtx,
        missing_shear_reference: Option<&str>,
    ) -> (CheckOutcome, Option<String>) {
        let material_issue =
            crate::material_strength::main_rebar_category_issue(ctx.rebar_material.as_ref())
                .map(|reason| format!("RC 検定: 断面「{}」の{reason}", sec.name));
        let shear_material_issue = crate::material_strength::shear_rebar_category_issue(
            sec,
            ctx.shear_rebar_material.as_ref(),
        );
        let main_rebar_valid = material_issue.is_none();
        let shear_rebar_valid = shear_material_issue.is_none();
        let shear_issue = missing_shear_reference.or_else(|| beam_shear_issue(forces, sec, ctx));
        let outcome = if let (true, true, Some(reason)) =
            (main_rebar_valid, shear_rebar_valid, missing_shear_reference)
        {
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
            )
        };
        let incomplete =
            combine_incomplete_reasons(material_issue, shear_material_issue.as_deref())
                .and_then(|reason| combine_incomplete_reasons(Some(reason), shear_issue));
        let incomplete = match sec.shape.as_ref() {
            Some(shape) if !shear_rebar_valid && missing_shear_reference.is_none() => {
                column::gravity_shear_reference_issues(ctx, forces.pos, shape).fold(
                    incomplete,
                    |reason, reference_issue| {
                        combine_incomplete_reasons(reason, Some(reference_issue))
                    },
                )
            }
            _ => incomplete,
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

    fn check_section(
        forces: &MemberForcesAt,
        sec: &Section,
        mat: &Material,
        ctx: &DesignCtx,
        main_rebar_valid: bool,
        shear_rebar_valid: bool,
        shear_issue: Option<&str>,
    ) -> CheckOutcome {
        let fc_raw = mat.fc.unwrap_or(0.0);
        if fc_raw <= 0.0 {
            return CheckOutcome::Skipped {
                reason: "RC 検定: Fc 未設定（Material.fc が None/0 です。コンクリート強度を設定してください）".to_string(),
            };
        }

        let new_rebar_unset = match &sec.shape {
            Some(SectionShape::RcBeamRect { rebar, .. }) => rebar.is_unset(),
            Some(SectionShape::RcColumnRect { rebar, .. }) => rebar.is_unset(),
            Some(SectionShape::RcColumnCircle { rebar, .. }) => rebar.is_unset(),
            _ => false,
        };
        if new_rebar_unset {
            return CheckOutcome::Skipped {
                reason: "RC 検定: 配筋が未入力です".to_string(),
            };
        }

        let shape = match &sec.shape {
            Some(
                s @ (SectionShape::RcBeamRect { .. }
                | SectionShape::RcColumnRect { .. }
                | SectionShape::RcColumnCircle { .. }),
            ) => s,
            _ => {
                return CheckOutcome::Skipped {
                    reason: "RC 検定: 配筋情報なし\
                             （Section.shape が RcBeamRect/\
                             RcColumnRect/RcColumnCircle ではありません）"
                        .to_string(),
                };
            }
        };

        if ctx.rebar_material.is_none() {
            return CheckOutcome::Skipped {
                reason: "RC 検定: 主筋の材料が未割当（断面タブで主筋の材料を割り当ててください）"
                    .to_string(),
            };
        }
        if ctx.shear_rebar_material.is_none() {
            return CheckOutcome::Skipped {
                reason: "RC 検定: せん断補強筋の材料が未割当\
                         （断面タブでせん断補強筋の材料を割り当ててください）"
                    .to_string(),
            };
        }
        if let Err(reason) = shape.validate_rebar_size_materials(
            ctx.rebar_material.as_ref().filter(|_| main_rebar_valid),
            ctx.shear_rebar_material
                .as_ref()
                .filter(|_| shear_rebar_valid),
        ) {
            return CheckOutcome::Skipped {
                reason: format!("RC 検定: {reason}"),
            };
        }
        if let Some(msg) = sepika_core::material_grade::shear_rebar_material_issue(
            ctx.shear_rebar_material
                .as_ref()
                .filter(|_| shear_rebar_valid),
        ) {
            return CheckOutcome::Skipped {
                reason: format!("RC 検定: {msg}"),
            };
        }

        if matches!(shape, SectionShape::RcBeamRect { .. }) && ctx.kind == MemberKind::Column {
            return CheckOutcome::Skipped {
                reason: "RC 検定: 梁用断面を柱部材に割り当てています（用途不一致）".to_string(),
            };
        }
        if matches!(
            shape,
            SectionShape::RcColumnRect { .. } | SectionShape::RcColumnCircle { .. }
        ) && matches!(ctx.kind, MemberKind::Girder | MemberKind::Brace)
        {
            return CheckOutcome::Skipped {
                reason: "RC 検定: 柱用断面を梁部材に割り当てています（用途不一致）".to_string(),
            };
        }

        let rebar_issue = match shape {
            SectionShape::RcBeamRect { b, d, rebar } => rebar.validate(*b, *d).err(),
            SectionShape::RcColumnRect { b, d, rebar } => rebar.validate(*b, *d).err(),
            SectionShape::RcColumnCircle { d, rebar } => rebar.validate(*d).err(),
            _ => None,
        };
        if let Some(e) = rebar_issue {
            return CheckOutcome::Skipped {
                reason: format!("RC 検定: 配筋が不整合です（{e}）"),
            };
        }

        if let (true, true, Some(reason)) = (main_rebar_valid, shear_rebar_valid, shear_issue) {
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
            ),
        }
    }
}

fn beam_shear_issue(
    forces: &MemberForcesAt,
    sec: &Section,
    ctx: &DesignCtx,
) -> Option<&'static str> {
    let qd = ctx.seismic_qd.as_ref()?;
    if !matches!(sec.shape, Some(SectionShape::RcBeamRect { .. })) {
        return None;
    }
    if !qd
        .long_at
        .iter()
        .find(|(p, _)| (p - forces.pos).abs() < 1e-6)
        .is_some_and(|(p, f)| p.is_finite() && f.iter().all(|v| v.is_finite()))
    {
        return Some("設計用せん断力に必要な重力参照内力が不足しています");
    }
    (qd.method != crate::QdMethod::Qd2 && !qd.q_simple.is_some_and(|q| q.is_finite() && q >= 0.0))
        .then_some("QD1用の部材荷重由来Q0が不足しています。FEMのQLでは代替しません")
}

#[cfg(test)]
mod tests;
