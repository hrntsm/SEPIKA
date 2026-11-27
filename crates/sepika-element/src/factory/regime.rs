//! フォースレジーム（`ForceRegime`）判定。
//!
//! - [`resolve_force_regime`] — `ForceRegime::Auto` をトポロジから判定
//! - [`ResolvedRegime`] — 判定結果（集中ばね / ファイバー）
//! - [`is_column_member`] — 柱用断面を持つ部材かどうか
//! - [`is_on_rigid_diaphragm`] — 剛床に所属するか

use sepika_core::model::{
    AnalysisKind, ElementData, ElementKind, ForceRegime, FrameSectionUse, Model,
};

/// 柱・大梁の解析方式。適用可否は入力診断と合わせて確認する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameAnalysisMethod {
    Elastic,
    ConcentratedSpring,
    Fiber,
}

pub(super) fn frame_analysis_method(
    data: &ElementData,
    model: &Model,
    kind: Option<AnalysisKind>,
) -> Option<FrameAnalysisMethod> {
    match data.kind {
        ElementKind::Beam | ElementKind::Fiber
            if kind.is_none() || crate::frame::beam::is_elastic_wall_girder(model, data) =>
        {
            Some(FrameAnalysisMethod::Elastic)
        }
        ElementKind::Beam => Some(match resolve_force_regime(data, model) {
            ResolvedRegime::ConcentratedSpring => FrameAnalysisMethod::ConcentratedSpring,
            ResolvedRegime::Fiber => FrameAnalysisMethod::Fiber,
        }),
        ElementKind::Fiber => Some(FrameAnalysisMethod::Fiber),
        _ => None,
    }
}

/// ForceRegime の自動選択結果
pub enum ResolvedRegime {
    ConcentratedSpring,
    Fiber,
}

/// ForceRegime::Auto をトポロジから判定する。
/// 剛床所属の梁 → ConcentratedSpring、それ以外 → Fiber
pub fn resolve_force_regime(data: &ElementData, model: &Model) -> ResolvedRegime {
    match data.force_regime {
        ForceRegime::UniaxialBendingShear => ResolvedRegime::ConcentratedSpring,
        ForceRegime::AxialBendingInteract => ResolvedRegime::Fiber,
        ForceRegime::Auto => {
            let is_column = is_column_member(data, model);
            let on_rigid_diaphragm = is_on_rigid_diaphragm(data, model);
            if on_rigid_diaphragm && !is_column {
                ResolvedRegime::ConcentratedSpring
            } else {
                ResolvedRegime::Fiber
            }
        }
    }
}

pub(super) fn is_column_member(data: &ElementData, model: &Model) -> bool {
    data.section
        .and_then(|id| model.sections.get(id.index()))
        .and_then(|section| section.frame_use)
        == Some(FrameSectionUse::Column)
}

fn is_on_rigid_diaphragm(data: &ElementData, model: &Model) -> bool {
    data.nodes.iter().any(|&n| model.node_on_rigid_diaphragm(n))
}
