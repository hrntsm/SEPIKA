//! 互換用の薄い再エクスポート。
//!
//! `SectionShape` 一族は `sepika_core::section_shape` へ移設された
//! （`core::Section` から参照できるようにするため）。既存コードが
//! `sepika_section::shape::SectionShape` を参照している箇所を壊さないよう、
//! ここでは再エクスポートのみ行う。実装・テストは移設先を参照。
pub use sepika_core::section_shape::*;
