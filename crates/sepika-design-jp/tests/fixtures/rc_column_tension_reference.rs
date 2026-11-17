pub fn tension_limit_n(long_term: bool) -> f64 {
    // 一様引張の釣合い。コンクリートの引張抵抗を除き、径19 mmの8本を215/345 N/mm²で評価する。
    if long_term {
        487_669.427_616_743_55
    } else {
        782_539.314_082_681_6
    }
}
