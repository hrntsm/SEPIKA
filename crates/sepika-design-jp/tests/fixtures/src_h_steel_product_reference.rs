// 純引張の釣合いを鉄骨と全主筋で、純曲げの累加を鋭角Hの矩形積分と引張主筋で求めた長期耐力。
pub const COLUMN_CAPACITY_N: f64 = 2_127_716.358_985_590_3;
pub const BEAM_CAPACITY_NMM: f64 = 378_434_601.464_927_43;

pub fn demand(column: bool) -> (f64, f64) {
    if column {
        (2_340_487.994_884_149_6, 0.0)
    } else {
        (0.0, 416_278_061.611_420_2)
    }
}

pub fn expected_ratio(column: bool) -> f64 {
    let (tension_n, moment_nmm) = demand(column);
    if column {
        tension_n / COLUMN_CAPACITY_N
    } else {
        moment_nmm / BEAM_CAPACITY_NMM
    }
}
