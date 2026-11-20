use super::multi_floor_model::{scale, HeadForces, UNIFORM_MM};

pub fn uniform_displacement_reactions() -> [HeadForces; 2] {
    // 曲げ・せん断のひずみエネルギーから得る両端固定梁の柔度。床の一様並進では床ひずみが0。
    let stiffness = |i_mm4: f64| {
        let length_mm: f64 = 3000.0;
        1.0 / (length_mm.powi(3) / (12.0 * 200_000.0 * i_mm4) + length_mm / (80_000.0 * 1_600.0))
    };
    let strong = stiffness(12_000_000.0);
    let weak = stiffness(8_000_000.0);
    let diagonal = (strong + weak) / 2.0;
    let cross = (strong - weak) / 2.0;
    [
        [
            [strong, 0.0],
            [strong, 0.0],
            [diagonal, -cross],
            [diagonal, cross],
        ],
        [
            [0.0, weak],
            [0.0, weak],
            [-cross, diagonal],
            [cross, diagonal],
        ],
    ]
    .map(|forces| scale(forces, UNIFORM_MM))
}
