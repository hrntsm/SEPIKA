//! 静的解析。
//!
//! - [`linear`] —   線形静的解析
//! - [`analysis`] — 地震・風の静的荷重生成と解析設定
pub mod analysis;
pub mod linear;

/// 構築時の要素ID、応答要素、自由度対応の順に保持する。
pub(crate) type BehaviorEntry = (
    sepika_core::ids::ElemId,
    Box<dyn sepika_element::behavior::ElementBehavior>,
    smallvec::SmallVec<[usize; 24]>,
);
