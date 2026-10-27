//! 部材ランク（FA..FD）の集約。
use super::holding_capacity::MemberRank;

/// ランクを 0(FA)..3(FD) の整数インデックスに変換する。
fn rank_index(r: MemberRank) -> u8 {
    match r {
        MemberRank::FA => 0,
        MemberRank::FB => 1,
        MemberRank::FC => 2,
        MemberRank::FD => 3,
    }
}

/// 整数インデックスをランクに変換する。インデックスが 3 を超える場合は FD を返す。
fn index_rank(i: u8) -> MemberRank {
    match i {
        0 => MemberRank::FA,
        1 => MemberRank::FB,
        2 => MemberRank::FC,
        _ => MemberRank::FD,
    }
}

/// 複数の部材ランクのうち最も不利（FD 寄り）なものを返す。`ranks` が空なら `None`。
///
/// 保有水平耐力（ルート3）の層ランク自動判定（UI-13）で、1 層に属する複数の
/// 鋼部材ランクから層の代表ランクを選ぶために使う。
pub fn worst_rank(ranks: &[MemberRank]) -> Option<MemberRank> {
    ranks.iter().map(|r| rank_index(*r)).max().map(index_rank)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 最も不利（FD 寄り）なランクの選択。空は `None`。
    #[test]
    fn test_worst_rank_cases() {
        for (ranks, expected) in [
            (
                vec![MemberRank::FA, MemberRank::FC, MemberRank::FB],
                Some(MemberRank::FC),
            ),
            (vec![MemberRank::FD, MemberRank::FA], Some(MemberRank::FD)),
            (vec![], None),
        ] {
            assert_eq!(worst_rank(&ranks), expected, "{ranks:?}");
        }
    }
}
