//! 側柱判定（自部材が耐震壁の側柱かどうかの幾何判定）。
//!
//! 判定結果として解放すべき局所曲げ面（[`ReleaseAxis`]）を返す。

use super::ReleaseAxis;
use crate::transform::LocalFrame;
use sepika_core::geom::vec3::dot;
use sepika_core::model::{ElementData, ElementKind, Model, WallSideColumnIndex};

/// 曲げを伝達する線材（柱・梁として扱う要素種別）か。
///
/// 大梁として壁の辺を構成しうるのは線材のみで、ブレース（軸材）・面要素・バネは
/// 曲げを伝達しないため除く。ファイバー梁・マルチスプリング梁は対象に含める。
pub fn is_line_member(kind: ElementKind) -> bool {
    matches!(
        kind,
        ElementKind::Beam | ElementKind::Fiber | ElementKind::MultiSpring
    )
}

/// 側柱の物理分類を適用できる線材の種別かを返す。
pub fn is_side_column_member(kind: ElementKind) -> bool {
    is_line_member(kind)
}

/// 共通物理分類で採用された側柱の、局所断面内の解放回転方向を返す。
/// 不正候補の理由は `WallSideColumnIndex::replacement_bindings` で取得できる。
pub fn wall_side_column_release(data: &ElementData, model: &Model) -> Option<ReleaseAxis> {
    SideColumnEdges::build(model).release_axis(data, model)
}

/// 壁面法線から解放すべき局所曲げ面を定める（回転軸が壁法線に平行な方）。
fn release_axis_for_normal(
    data: &ElementData,
    p0: [f64; 3],
    p1: [f64; 3],
    normal: [f64; 3],
) -> ReleaseAxis {
    let axis = LocalFrame::from_nodes(p0, p1, data.local_axis.ref_vector)
        .unwrap_or_else(|reason| panic!("部材 ID {} の局所座標: {reason}", data.id.0));
    let ny = dot(axis.rot[1], normal);
    let nz = dot(axis.rot[2], normal);
    let norm = ny.hypot(nz);
    ReleaseAxis::LocalDirection([ny / norm, nz / norm])
}

/// 共通物理分類による壁側柱の事前インデックス。
pub struct SideColumnEdges {
    index: WallSideColumnIndex,
}

impl SideColumnEdges {
    pub fn build(model: &Model) -> Self {
        Self {
            index: WallSideColumnIndex::build(model),
        }
    }

    /// 共通分類の不正候補は採用しない。理由は `classification` から取得できる。
    pub fn release_axis(&self, data: &ElementData, model: &Model) -> Option<ReleaseAxis> {
        if !is_side_column_member(data.kind) || data.nodes.len() != 2 {
            return None;
        }
        let bindings = self.index.replacement_bindings(data.id).ok()?;
        let binding = bindings.first()?;
        if data.nodes.as_slice() != binding.edge
            && data.nodes.as_slice() != [binding.edge[1], binding.edge[0]]
        {
            return None;
        }
        let p0 = model.nodes.get(data.nodes[0].index())?.coord;
        let p1 = model.nodes.get(data.nodes[1].index())?.coord;
        let mut normal = binding.normal;
        if normal
            .iter()
            .find(|v| v.abs() > 1e-12)
            .is_some_and(|v| *v < 0.0)
        {
            normal = normal.map(|v| -v);
        }
        Some(release_axis_for_normal(data, p0, p1, normal))
    }

    pub fn classification(&self) -> &WallSideColumnIndex {
        &self.index
    }
}
