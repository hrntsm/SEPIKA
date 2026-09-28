//! 取り付く床板（片持ち・バルコニー・出隅）の分配戦略。
//!
//! - [`distribute_to_node`] — 取り付く床板の荷重を節点（柱）へ集中
//! - [`distribute_cantilever`] — 取付き大梁へ出し幅に応じた等分布で分配

use squid_n_core::geom::polygon::area_xy;
use squid_n_core::ids::ElemId;
use squid_n_core::ids::NodeId;

use super::fem::fem_uniform;
use super::geometry::edge_len;
use super::types::{push_edge, BeamLoad, Cmq, LoadShape, LoadTarget};

/// 取り付く床板の荷重を節点（柱）へ集中させる分配。
///
/// 出隅の片持ちスラブは、荷重伝達方向および片持ち梁の取付きに関わらず、節点荷重として
/// すべて柱に伝達する。本実装ではこれに従い、全荷重 `W = w × 多角形面積`
/// （[`area_xy`]。構造芯から出隅先端までの長方形＝境界そのものの面積）に
/// `ratio` を掛けた分を、`node` への単一の集中荷重として返す。
/// 小梁反力や取り付く壁版の柱集中と同じ `LoadTarget::Node` + `LoadShape::Point`
/// （`q_i = W`、`q_j = 0`）の機構を再利用する。
///
/// `ratio` は呼び出し側が渡す按分比。
/// [`LoadTransfer::Columns`](squid_n_core::model::LoadTransfer::Columns) は取付き線の
/// 区間中点 `t_mid` から `1.0 - t_mid`／`t_mid` を渡す（全長 `[0, 1]` なら `t_mid = 0.5`
/// で両端とも 0.5）。出隅
/// （[`RegionAnchor::Point`](squid_n_core::model::RegionAnchor::Point)）は全荷重を
/// 渡すため 1.0 を用いる。
pub(crate) fn distribute_to_node(
    node: NodeId,
    coords: &[[f64; 3]],
    w: f64,
    ratio: f64,
    loads: &mut Vec<BeamLoad>,
) {
    let area = area_xy(coords);
    if area <= 0.0 {
        return;
    }
    let total = w * area * ratio;
    loads.push(BeamLoad {
        elem: ElemId(u32::MAX),
        target: LoadTarget::Node(node),
        shape: LoadShape::Point { p: total, x: 0.0 },
        cmq: Cmq {
            c_i: 0.0,
            c_j: 0.0,
            q_i: total,
            q_j: 0.0,
        },
    });
}

/// 取り付く床板の分配（取付き線へ分布）。
///
/// 取付き線（辺0）の取付き大梁だけへ、`w_line = w·d` の等分布を載せる。
pub(crate) fn distribute_cantilever(coords: &[[f64; 3]], w: f64, loads: &mut Vec<BeamLoad>) {
    if coords.len() < 4 {
        return;
    }
    let l_attach = edge_len(coords, 0);
    if l_attach <= 1e-9 {
        return;
    }
    let edge = [coords[1][0] - coords[0][0], coords[1][1] - coords[0][1]];
    let edge_norm = (edge[0] * edge[0] + edge[1] * edge[1]).sqrt();
    let distance = |p: [f64; 3]| {
        ((p[0] - coords[0][0]) * edge[1] - (p[1] - coords[0][1]) * edge[0]).abs() / edge_norm
    };
    let d = 0.5 * (distance(coords[2]) + distance(coords[3]));
    if d <= 1e-9 {
        return;
    }

    let w_line = w * d;
    push_edge(
        loads,
        0,
        LoadShape::Uniform { w: w_line },
        fem_uniform(w_line, l_attach),
    );
}
