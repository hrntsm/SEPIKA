pub fn axial_limit_n(circle: bool) -> f64 {
    // 一様圧縮時の換算面積。8本×π×9.5²、fc=8、n=15。鉄筋応力120<215 N/mm²。
    let area_mm2 = if circle {
        std::f64::consts::PI * 90_000.0
    } else {
        240_000.0
    };
    8.0 * (area_mm2 + 14.0 * (8.0 * std::f64::consts::PI * 9.5_f64.powi(2)))
}
