//! 幾何ユーティリティ（座標計算）。
//!
//! - [`dist3`] — 2 点間の 3D 距離
//! - `is_vertical_pair` — 両端が鉛直材（柱）かの判定
//!   （[`sepika_core::geom::is_vertical_pair`] の再エクスポート）

/// 2 点間の 3D 距離 [mm]。算定の情報源は `sepika-core` に置く。
pub(super) use sepika_core::geom::vec3::dist as dist3;

/// 「鉛直材（柱）」判定。仕上げ周長式・柱脚梁せい付加・壁領域の構面走査・通り芯の
/// 自動生成が共通で用いるため、判定規則は `sepika-core` を情報源とする。
pub(super) use sepika_core::geom::is_vertical_pair;
