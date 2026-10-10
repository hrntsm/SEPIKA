//! RC 造耐震壁の**せん断非線形特性（トリリニア）**。
//!
//! 非線形解析で用いるせん断ばねの骨格曲線（トリリニア）を算定する。
//! 原点・ひび割れ・終局の3点を求める。入力は SI 系で受け取り、
//! Qc の評価は内部で工学単位系へ換算する。

/// 単位換算: 1 kgf/cm² = 0.0980665 N/mm²。N/mm² → kgf/cm² は逆数を乗じる。
const NMM2_TO_KGFCM2: f64 = 1.0 / 0.0980665;
/// 単位換算: 1 kgf = 1 kg × 標準重力加速度 g [m/s²] = 9.80665 N。
/// g の情報源は `sepika_core::units`（内部単位系の mm/s² から m/s² へ換算）。
const KGF_TO_N: f64 = sepika_core::units::GRAVITY_MM_S2 / 1000.0;

/// RC 造耐震壁のせん断トリリニア算定の入力。
///
/// 単位は SI 系（長さ [mm]・面積 [mm²]・応力 [N/mm²]・軸力 [N]）で統一する。
#[derive(Clone, Copy, Debug)]
pub struct WallShearTrilinearInput {
    /// 壁高さ（上下梁中心間）[mm]。
    pub wall_height_mm: f64,
    /// 壁長さ（側柱中心間）[mm]。
    pub wall_length_mm: f64,
    /// コンクリート設計基準強度 Fc [N/mm²]。
    pub fc: f64,
    /// 壁体断面積 Aw [mm²]（側柱＋壁板の軸断面積。Qc・σ0 の基準面積）。
    pub aw: f64,
    /// 引張側最端の柱 1 本の主筋量 [mm²]（pg = 100·この値/Aw [%]）。
    pub tension_column_main_area: f64,
    /// 等価壁厚 te [mm]（I 形断面を等価長方形に置換した幅。壁厚 t の 1.5 倍以下）。
    pub te: f64,
    /// 壁厚 t [mm]（pwh = Pwh·t/te の換算に用いる）。
    pub t: f64,
    /// 付帯柱を含めた耐震壁の全長 D [mm]。
    pub d_wall: f64,
    /// 圧縮側柱のせい Dc [mm]（有効せい d = D − Dc/2）。
    pub dc_compression: f64,
    /// 引張側柱の主筋断面積 at [mm²]（pte = 100·at/(te·d) の分子）。
    pub tension_column_at: f64,
    /// 水平せん断補強筋（横筋）の材料強度 σwh [N/mm²]。
    pub sigma_wh: f64,
    /// 実壁厚に対する横筋比（小数）。βsでは等価壁厚へ換算しない。
    pub pwh_ratio: f64,
    /// 平均圧縮軸応力度 [N/mm²]。
    pub sigma_0: f64,
    /// せん断スパン比 M/(QD)。骨格では有限の正値が必須。
    pub shear_span_ratio: f64,
    /// 開口 (幅, 高さ, 壁高さ, 柱心距離) [mm]。骨格は None のみ対応。
    pub opening: Option<(f64, f64, f64, f64)>,
}

/// 純せん断のひび割れ・終局点の耐力と終局割線剛性比。
#[derive(Clone, Copy, Debug)]
pub struct WallShearTrilinear {
    /// ひび割れ耐力 [N]。
    pub qc: f64,
    /// 終局耐力 [N]。
    pub qu: f64,
    /// 終局点の割線剛性/純せん断初期剛性。
    pub beta_s: f64,
    /// 開口低減率。無開口では 1。
    pub r_opening: f64,
}

impl WallShearTrilinear {
    /// 純せん断の初期剛性 [N] に対する (せん断角, Q [N])。不正な剛性・骨格はエラー。
    pub fn skeleton_points(&self, k_elastic: f64) -> Result<[(f64, f64); 3], String> {
        if !k_elastic.is_finite() || k_elastic <= 0.0 {
            return Err("純せん断初期剛性は有限の正値が必要です".into());
        }
        if !self.qc.is_finite()
            || !self.qu.is_finite()
            || !self.beta_s.is_finite()
            || self.qc <= 0.0
            || self.qu <= self.qc
            || !(0.0..1.0).contains(&self.beta_s)
        {
            return Err("ひび割れ点・終局点・剛性低下率が骨格の適用条件を満たしません".into());
        }
        let gamma_c = self.qc / k_elastic;
        let gamma_u = self.qu / (self.beta_s * k_elastic);
        if !gamma_c.is_finite() || gamma_c <= 0.0 || !gamma_u.is_finite() || gamma_u <= gamma_c {
            return Err("終局せん断変形を有限かつひび割れ変形より大きく算定できません".into());
        }
        Ok([(0.0, 0.0), (gamma_c, self.qc), (gamma_u, self.qu)])
    }

    /// G [N/mm²]・せん断有効面積 [mm²]・高さ [mm] に対する (せん断変位 [mm], Q [N])。
    /// 面積は形状係数を反映済みの純せん断成分。曲げ柔性を含めない。不正入力はエラー。
    pub fn displacement_points(
        &self,
        shear_mpa: f64,
        shear_area_mm2: f64,
        height_mm: f64,
    ) -> Result<[(f64, f64); 3], String> {
        if [shear_mpa, shear_area_mm2, height_mm]
            .iter()
            .any(|v| !v.is_finite() || *v <= 0.0)
        {
            return Err("G・せん断有効面積・高さは有限の正値が必要です".into());
        }
        self.skeleton_points(shear_mpa * shear_area_mm2 / height_mm)
    }
}

/// せん断ひび割れ耐力 [N]。Fc [N/mm²]・Aw [mm²]・側柱主筋面積 [mm²] を用いる。
/// FcまたはAwが非正の場合は0。
pub fn wall_shear_crack(inp: &WallShearTrilinearInput) -> f64 {
    if inp.fc <= 0.0 || inp.aw <= 0.0 {
        return 0.0;
    }
    let fc_kgf = inp.fc * NMM2_TO_KGFCM2;
    let aw_cm2 = inp.aw / 100.0;
    let pg_pct = 100.0 * inp.tension_column_main_area.max(0.0) / inp.aw;
    let qc_kgf = (0.043 * pg_pct + 0.051) * fc_kgf * aw_cm2;
    qc_kgf * KGF_TO_N
}

/// 横筋比（実壁厚基準）・横筋降伏強度/Fc [N/mm²] から終局割線剛性比を求める。
/// 強度は有限正、比は有限かつ 0〜0.015。Fc の原研究範囲外はエラー。
pub fn wall_shear_beta_s(inp: &WallShearTrilinearInput) -> Result<f64, String> {
    if !inp.fc.is_finite() || !(170.0 * 0.0980665..=280.0 * 0.0980665).contains(&inp.fc) {
        return Err("Fc が剛性低下率の原研究範囲（170〜280 kgf/cm²）外です".into());
    }
    if !inp.sigma_wh.is_finite() || inp.sigma_wh <= 0.0 {
        return Err("横筋の降伏強度 fy は有限の正値が必要です".into());
    }
    if !inp.pwh_ratio.is_finite() || !(0.0..=0.015).contains(&inp.pwh_ratio) {
        return Err("横筋比 pwh_ratio は有限かつ 0〜0.015 が必要です".into());
    }
    let beta = 0.46 * inp.pwh_ratio * inp.sigma_wh / inp.fc + 0.14;
    if !(0.0..1.0).contains(&beta) {
        return Err("横筋による剛性低下率が 0〜1 の適用域を満たしません".into());
    }
    Ok(beta)
}

/// 開口低減率 r（無次元、耐震壁。RC 終局強度設計資料）。
///
/// `r = 1 − max(r0, l0/lw, h0/h)`、`r0 = √(h0·l0/(h·lw))`。
/// 無開口（`opening == None`）は 1.0。極端な開口で負になる場合は 0 にクランプする。
pub fn wall_shear_opening_reduction(opening: Option<(f64, f64, f64, f64)>) -> f64 {
    sepika_core::rc_wall_capacity::wall_opening_reduction_strength(opening)
}

/// 終局せん断耐力 [N]。横筋比は実壁厚基準で入力し、耐力式で等価壁厚換算する。
/// 開口低減を含む耐力単体評価。骨格の適用条件は一括算定入口で別途検査する。
pub fn wall_shear_ultimate(inp: &WallShearTrilinearInput) -> f64 {
    sepika_core::rc_wall_capacity::wall_shear_ultimate(
        &sepika_core::rc_wall_capacity::RcWallShearInput {
            fc: inp.fc,
            te: inp.te,
            t: inp.t,
            d_wall: inp.d_wall,
            dc_compression: inp.dc_compression,
            tension_column_at: inp.tension_column_at,
            sigma_wh: inp.sigma_wh,
            pwh_ratio: inp.pwh_ratio,
            sigma_0: inp.sigma_0,
            shear_span_ratio: inp.shear_span_ratio,
            opening: inp.opening,
        },
    )
}

/// 無開口壁の Qc・βs・Qu を算定する。不正入力・Qc≥Qu はエラー。
pub fn wall_shear_trilinear(inp: &WallShearTrilinearInput) -> Result<WallShearTrilinear, String> {
    let beta_s = wall_shear_beta_s(inp)?;
    if !inp.wall_height_mm.is_finite()
        || !inp.wall_length_mm.is_finite()
        || inp.wall_height_mm <= 0.0
        || inp.wall_length_mm <= 0.0
        || !(0.33..=1.63).contains(&(inp.wall_height_mm / inp.wall_length_mm))
    {
        return Err("壁高さ/壁長が原研究範囲 0.33〜1.63 外です".into());
    }
    if inp.opening.is_some() {
        return Err("有開口壁のせん断トリリニア骨格は未対応です".into());
    }
    if [
        inp.aw,
        inp.tension_column_main_area,
        inp.te,
        inp.t,
        inp.d_wall,
        inp.dc_compression,
        inp.tension_column_at,
    ]
    .iter()
    .any(|v| !v.is_finite() || *v <= 0.0)
        || !inp.sigma_0.is_finite()
    {
        return Err("せん断耐力の断面・材料入力が有限の有効値ではありません".into());
    }
    if !inp.shear_span_ratio.is_finite() || inp.shear_span_ratio <= 0.0 {
        return Err("せん断スパン比 M/(QD) が有限の正値ではありません".into());
    }
    let qu = wall_shear_ultimate(inp);
    let qc = wall_shear_crack(inp);
    if !qu.is_finite() || !qc.is_finite() || qc <= 0.0 || qu <= qc {
        return Err("Qc < Qu を満たさないためせん断トリリニア骨格を算定できません".into());
    }
    Ok(WallShearTrilinear {
        qc,
        qu,
        beta_s,
        r_opening: 1.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn base_input() -> WallShearTrilinearInput {
        WallShearTrilinearInput {
            wall_height_mm: 3000.0,
            wall_length_mm: 4000.0,
            fc: 24.0,
            aw: 1_440_000.0,
            tension_column_main_area: 3097.0,
            te: 180.0,
            t: 180.0,
            d_wall: 4600.0,
            dc_compression: 600.0,
            tension_column_at: 3097.0,
            sigma_wh: 295.0,
            pwh_ratio: 0.002,
            sigma_0: 1.0,
            shear_span_ratio: 1.5,
            opening: None,
        }
    }
    #[test]
    fn horizontal_rebar_beta_matches_independent_expected_values() {
        let mut input = base_input();
        assert!((wall_shear_beta_s(&input).unwrap() - 0.1513083333333333).abs() < 1e-12);
        input.pwh_ratio = 0.006;
        assert!((wall_shear_beta_s(&input).unwrap() - 0.173925).abs() < 1e-12);
        input.pwh_ratio = 0.004;
        input.sigma_wh = 345.0;
        assert!((wall_shear_beta_s(&input).unwrap() - 0.16645).abs() < 1e-12);
        input.te = 270.0;
        assert!((wall_shear_beta_s(&input).unwrap() - 0.16645).abs() < 1e-12);
    }
    #[test]
    fn invalid_horizontal_strength_and_ratio_are_errors() {
        for strength in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let mut input = base_input();
            input.sigma_wh = strength;
            assert!(wall_shear_beta_s(&input).is_err());
        }
        for ratio in [-0.001, f64::NAN, f64::INFINITY, 0.016] {
            let mut input = base_input();
            input.pwh_ratio = ratio;
            assert!(wall_shear_beta_s(&input).is_err());
        }
    }
    #[test]
    fn skeleton_secant_and_interval_stiffness_are_distinct() {
        let tri = WallShearTrilinear {
            qc: 100_000.0,
            qu: 600_000.0,
            beta_s: 0.15,
            r_opening: 1.0,
        };
        // 矩形の As=A/1.2=600000 mm²、G=10000、h=3000。K0=2000000 N/mm。
        let points = tri
            .displacement_points(10_000.0, 600_000.0, 3000.0)
            .unwrap();
        assert_eq!(points, [(0.0, 0.0), (0.05, 100_000.0), (2.0, 600_000.0)]);
        let angle = tri.skeleton_points(6_000_000_000.0).unwrap();
        assert!((angle[2].0 - 0.0006666666666666666).abs() < 1e-15);
        assert!(
            ((points[2].1 - points[1].1) / (points[2].0 - points[1].0) - 256410.2564102564).abs()
                < 1e-8
        );
        for stiffness in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(tri.skeleton_points(stiffness).is_err());
        }
    }
    #[test]
    fn crack_force_matches_independent_unit_conversion() {
        assert!((wall_shear_crack(&base_input()) - 2_082_170.4).abs() < 1e-6);
    }
    #[test]
    fn full_shear_skeleton_matches_independent_hand_calculation() {
        let mut input = base_input();
        input.fc = 17.0;
        input.aw = 1_332_000.0;
        input.te = 270.0;
        input.pwh_ratio = 0.015;
        let properties = sepika_core::section_shape::wall_rectangular_section_properties(
            4000.0,
            180.0,
            [Some([600.0, 600.0]); 2],
        )
        .unwrap();
        assert_eq!(properties.area, 1_332_000.0);
        assert!((properties.shear_area - 783_187.0050213936).abs() < 1e-6);
        let tri = wall_shear_trilinear(&input).unwrap();
        assert!((tri.qc - 1_381_234.7).abs() < 1e-6);
        assert!((tri.qu - 2_443_056.1431899336).abs() < 1e-6);
        assert!((tri.beta_s - 0.25973529411764706).abs() < 1e-12);
        let points = tri
            .displacement_points(10_000.0, properties.shear_area, 3000.0)
            .unwrap();
        assert!((points[1].0 - 0.5290823358192479).abs() < 1e-12);
        assert!((points[2].0 - 3.602950218988746).abs() < 1e-12);
    }

    #[test]
    fn unsupported_skeleton_is_not_rounded_into_success() {
        let mut input = base_input();
        input.opening = Some((200.0, 200.0, 3000.0, 4000.0));
        assert!(wall_shear_trilinear(&input).is_err());
        input.opening = None;
        input.tension_column_main_area = 1e6;
        assert!(wall_shear_trilinear(&input).is_err());
    }

    #[test]
    fn skeleton_requires_finite_positive_shear_span_ratio() {
        let mut input = base_input();
        input.fc = 17.0;
        input.te = 270.0;
        input.pwh_ratio = 0.015;
        assert!(wall_shear_trilinear(&input).is_ok());
        for ratio in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            input.shear_span_ratio = ratio;
            assert!(matches!(wall_shear_trilinear(&input), Err(reason)
                if reason.contains("せん断スパン比") && reason.contains("有限の正値")));
        }
        input.shear_span_ratio = 0.5;
        let below_range = wall_shear_trilinear(&input).unwrap();
        input.shear_span_ratio = 1.0;
        let lower_bound = wall_shear_trilinear(&input).unwrap();
        assert_eq!(below_range.qu, lower_bound.qu);
    }
}
