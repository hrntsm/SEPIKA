//! RC 矩形断面の簡易終局耐力算定（部材ランク判定用）— 実装本体は sepika-core に移設。
//!
//! `sepika_solver::nonlinear::pushover` のせん断降伏判定（Qy の荒川式化）でも同じ算定式
//! （`rc_qsu_simple` 等）を使うが、sepika-solver（Layer 4）は sepika-design-jp
//! （Layer 5）に依存できない（循環依存になる）ため、実装本体は Layer 0 の
//! `sepika_core::rc_capacity` へ移設した。本モジュールは既存呼び出し
//! （`sepika_design_jp::secondary::rc_capacity::{rc_qsu_simple, RcCapacityInput}` 等、
//! 例: `sepika_core::rc_capacity::rc_capacity_input_from_rect`）を無修正で維持するための
//! 再エクスポートのみを行う。
pub use sepika_core::rc_capacity::*;
