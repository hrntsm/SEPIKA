//! **終局検定（保有水平耐力計算における部材の終局強度検定）**。
//!
//! 矩形RC梁・柱では、確定総応答による終局せん断と、大地震時の付着応力度・
//! カットオフ筋の必要延長を照査する。円形RC柱は軸・曲げの終局耐力を扱う。
//!
//! CFT 柱の軸終局耐力は [`cft`]、
//! 柱梁接合部の終局耐力は [`crate::ultimate::joint`] を参照。

#[cfg(test)]
use crate::MemberKind;
#[cfg(test)]
use sepika_core::model::Model;

pub mod cft;
pub mod cft_nm;
pub mod joint;
pub mod rc_axial;
mod rc_bond;

mod cft_check;
mod demand_diagnostic;
mod options;
mod rc_check;
#[allow(dead_code)]
pub(crate) mod rc_props;
mod rc_strength;

pub use cft::{
    cft_axial_ultimate, cft_column_class, cft_concrete_buckling_axial,
    cft_concrete_buckling_stress, cft_concrete_slenderness, cft_ncu1, CftAxialInput,
    CftAxialUltimate, CftColumnClass, CftLongBuckling,
};
pub(crate) use cft_check::cft_circular_qd1_mu;
pub use cft_check::{cft_mu_nm, collect_cft_ultimate_checks, CftUltimateCheck};
pub use cft_nm::{
    cft_long_medium_column_mu, cft_nk, cft_short_column_mu, CftBendingInput, CftLongMediumInput,
};
pub use joint::{
    joint_fj, joint_kappa, rc_joint_ultimate, RcJointUltimateInput, RcJointUltimateResult,
};
pub use options::{
    MemberDemand, StaticBeamBending, StaticBeamHingeMechanism, UltimateDemandMode,
    UltimateShearOptions,
};
pub use rc_axial::{rc_axial_margin, rc_column_axial_ultimate, RcAxialUltimate};
pub use rc_bond::{
    RcUltimateBondAxis, RcUltimateBondBar, RcUltimateBondConfinement, RcUltimateBondDemand,
    RcUltimateBondElasticStress, RcUltimateBondInput, RcUltimateBondMomentDistribution,
    RcUltimateBondPlacement, RcUltimateBondStress, RcUltimateBondTransverseRebar,
    RcUltimateCutoffBarStress, RcUltimateThroughBarYield,
};
pub use rc_check::{collect_rc_ultimate_checks, UltimateCheck};
pub use rc_strength::biaxial_margin;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod gravity_demand_contract_tests;
