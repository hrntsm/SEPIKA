//! 断面・材料および荷重（節点/部材荷重）の編集コマンド。
//!
//! - [`section`] — 断面の追加・削除・複製・形状/プロパティ編集。
//! - [`material`] — 材料の追加・削除・プロパティ編集。
//! - [`element_assign`] — 部材への断面・材料・履歴則・制振ダンパーの割当。
//! - [`loads`] — 荷重ケース名・節点荷重・部材荷重の編集。
//! - [`damper_def`] — 制振ダンパー定義（プリセットライブラリ）の追加・更新・削除。

use super::*;

fn horizontal_primary_cft(
    model: &Model,
    elem: &squid_n_core::model::ElementData,
    shape: &squid_n_section::shape::SectionShape,
) -> bool {
    elem.kind == squid_n_core::model::ElementKind::Beam
        && elem
            .nodes
            .first()
            .and_then(|a| elem.nodes.get(1).map(|b| (a, b)))
            .and_then(|(a, b)| Some((model.nodes.get(a.index())?, model.nodes.get(b.index())?)))
            .is_some_and(|(a, b)| !squid_n_core::geom::is_vertical_axis(a.coord, b.coord))
        && matches!(
            shape,
            squid_n_section::shape::SectionShape::CftBox { .. }
                | squid_n_section::shape::SectionShape::CftPipe { .. }
        )
}

mod damper_def;
mod element_assign;
mod loads;
mod material;
mod section;

pub use damper_def::*;
pub use element_assign::*;
pub use loads::*;
pub use material::*;
pub use section::*;
