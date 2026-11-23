//! SRC 造の部材種別（保有水平耐力計算の Ds 算定、技術基準解説書 表 2.6.6-5）。
//!
//! - [`src_column_rank`] — 与えた耐力比と破壊状況による SRC 柱の表引き
//! - [`src_wall_type`] — SRC 耐震壁の種別（せん断破壊 WC・それ以外 WA）

use super::holding_capacity::MemberRank;

/// SRC 柱の表 2.6.6-5 を、圧縮力と圧縮耐力の比 `n_over_n0`、鉄骨部分と柱全体の曲げ耐力の比 `smo_over_m0` で引く。
/// `shear_failure` は崩壊形における破壊状況を表す（せん断破壊は `true`、曲げ破壊は `false`）。
/// 告示判定に使う場合は、呼び出し側で耐力比と破壊状況の適用根拠を確認する。
pub fn src_column_rank(n_over_n0: f64, smo_over_m0: f64, shear_failure: bool) -> MemberRank {
    if n_over_n0 > 0.4 {
        return MemberRank::FD;
    }
    let base: u8 = match (n_over_n0 > 0.3, smo_over_m0 < 0.4) {
        (false, false) => 0,
        (false, true) | (true, false) => 1,
        (true, true) => 2,
    };
    let idx = base + u8::from(shear_failure);
    match idx {
        0 => MemberRank::FA,
        1 => MemberRank::FB,
        2 => MemberRank::FC,
        _ => MemberRank::FD,
    }
}

/// SRC 耐震壁の種別。破壊モードがせん断破壊の場合を WC、それ以外を WA とする。
///
/// 返り値は Ds 表の行選択に用いるグループ表現（0=A(WA)…2=C(WC)。
/// [`super::ds_group`] の規約と同じく [`MemberRank`] のインデックスで表す）。
pub fn src_wall_type(shear_failure: bool) -> MemberRank {
    if shear_failure {
        MemberRank::FC
    } else {
        MemberRank::FA
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 表 2.6.6-5 の全区分（曲げ破壊・せん断破壊 × 5 列）を検証する。
    #[test]
    fn test_src_column_rank_table_2_6_6_5() {
        assert_eq!(src_column_rank(0.2, 0.5, false), MemberRank::FA);
        assert_eq!(src_column_rank(0.2, 0.3, false), MemberRank::FB);
        assert_eq!(src_column_rank(0.35, 0.5, false), MemberRank::FB);
        assert_eq!(src_column_rank(0.35, 0.3, false), MemberRank::FC);
        assert_eq!(src_column_rank(0.5, 0.5, false), MemberRank::FD);
        assert_eq!(src_column_rank(0.2, 0.5, true), MemberRank::FB);
        assert_eq!(src_column_rank(0.2, 0.3, true), MemberRank::FC);
        assert_eq!(src_column_rank(0.35, 0.5, true), MemberRank::FC);
        assert_eq!(src_column_rank(0.35, 0.3, true), MemberRank::FD);
        assert_eq!(src_column_rank(0.5, 0.3, true), MemberRank::FD);
    }

    /// 境界値: N/N0=0.3・0.4、sM0/M0=0.4 は「以下」「以上」側（有利側）に入る。
    #[test]
    fn test_src_column_rank_boundaries() {
        assert_eq!(src_column_rank(0.3, 0.4, false), MemberRank::FA);
        assert_eq!(src_column_rank(0.4, 0.4, false), MemberRank::FB);
        assert_eq!(src_column_rank(0.4, 0.39, false), MemberRank::FC);
    }

    #[test]
    fn test_src_wall_type() {
        assert_eq!(src_wall_type(false), MemberRank::FA, "WA");
        assert_eq!(src_wall_type(true), MemberRank::FC, "WC");
    }
}
