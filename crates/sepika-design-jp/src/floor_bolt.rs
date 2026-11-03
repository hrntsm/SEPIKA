use sepika_core::model::{HighStrengthBolt, SecondaryBeamBoltConnection};

/// 端番号順のせん断伝達検定。未検定理由がある場合は総合適合としない。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BeamBoltResult {
    pub symbol: String,
    pub input: Option<SecondaryBeamBoltConnection>,
    pub q_n: [f64; 2],
    pub supported: [bool; 2],
    pub qa_per_bolt_n: Option<f64>,
    pub qa_n: Option<f64>,
    pub ratios: [Option<f64>; 2],
    pub reasons: Vec<String>,
}

impl BeamBoltResult {
    pub fn complete(&self) -> bool {
        self.reasons.is_empty() && self.ratios.iter().flatten().count() > 0
    }
    pub fn max_ratio(&self) -> Option<f64> {
        self.ratios.iter().flatten().copied().reduce(f64::max)
    }
}

/// 長期許容せん断力 [N/本]。径・面数・必要な接合条件の不備は理由を返す。
pub fn allowable_bolt_shear(input: &SecondaryBeamBoltConnection) -> Result<f64, String> {
    if ![1, 2].contains(&input.friction_surfaces) {
        return Err("摩擦面数は1面または2面が必要です".into());
    }
    if !input.surface_treatment_confirmed {
        return Err("所定の摩擦面処理・すべり係数が未確認です".into());
    }
    if !input.concentric_shear_only_confirmed {
        return Err("外力による引張なし・偏心付加力不要の接合条件が未確認です".into());
    }
    let d = input.diameter_mm;
    match input.bolt {
        HighStrengthBolt::F10T => {
            if ![12, 16, 20, 22, 24, 27, 30].contains(&d) {
                return Err("F10Tの対応径はM12・16・20・22・24・27・30です".into());
            }
            Ok(150.0 * std::f64::consts::PI * f64::from(d).powi(2) / 4.0
                * f64::from(input.friction_surfaces))
        }
        HighStrengthBolt::F8TGalvanized => {
            let values = match d {
                16 => [22_700.0, 45_400.0],
                20 => [35_400.0, 70_800.0],
                22 => [44_000.0, 88_000.0],
                24 => [51_200.0, 102_000.0],
                27 => [66_600.0, 133_000.0],
                30 => [81_300.0, 163_000.0],
                _ => {
                    return Err("F8T（溶融亜鉛めっき）の対応径はM16・20・22・24・27・30です".into())
                }
            };
            Ok(values[(input.friction_surfaces - 1) as usize])
        }
    }
}

/// 各端の反力 `[N]` と支持端フラグから検定する。片持ち支持端は曲げ伝達が対象外。
pub fn design_beam_bolts(
    symbol: &str,
    input: Option<&SecondaryBeamBoltConnection>,
    q_n: [f64; 2],
    supported: [bool; 2],
    cantilever: bool,
) -> BeamBoltResult {
    let mut result = BeamBoltResult {
        symbol: symbol.into(),
        input: input.cloned(),
        q_n,
        supported,
        qa_per_bolt_n: None,
        qa_n: None,
        ratios: [None; 2],
        reasons: Vec::new(),
    };
    let Some(input) = input else {
        result
            .reasons
            .push("断面符号のボルト接合条件が未設定です".into());
        return result;
    };
    if input.count == 0 {
        result
            .reasons
            .push("片端のボルト本数は正の整数が必要です".into());
    }
    if !q_n.iter().all(|q| q.is_finite()) {
        result
            .reasons
            .push("端部せん断力が有限値ではありません".into());
    }
    match allowable_bolt_shear(input) {
        Ok(qa) => {
            result.qa_per_bolt_n = Some(qa);
            if result.reasons.is_empty() {
                let capacity = qa * f64::from(input.count);
                result.qa_n = Some(capacity);
                for end in 0..2 {
                    if supported[end] {
                        result.ratios[end] = Some(q_n[end].abs() / capacity);
                    }
                }
            }
        }
        Err(reason) => result.reasons.push(reason),
    }
    if !supported.iter().any(|v| *v) {
        result.reasons.push("支持端がありません".into());
    }
    if cantilever {
        result
            .reasons
            .push("片持ち支持端の曲げ伝達・引張は対象外です。せん断比は参考値です".into());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input(bolt: HighStrengthBolt, surfaces: u32) -> SecondaryBeamBoltConnection {
        SecondaryBeamBoltConnection {
            bolt,
            diameter_mm: 20,
            friction_surfaces: surfaces,
            count: 2,
            surface_treatment_confirmed: true,
            concentric_shear_only_confirmed: true,
        }
    }
    #[test]
    fn published_long_term_values_cover_both_grades_and_surfaces() {
        for (bolt, surfaces, expected, tolerance) in [
            (HighStrengthBolt::F10T, 1, 47_100.0, 30.0),
            (HighStrengthBolt::F10T, 2, 94_200.0, 60.0),
            (HighStrengthBolt::F8TGalvanized, 1, 35_400.0, 0.001),
            (HighStrengthBolt::F8TGalvanized, 2, 70_800.0, 0.001),
        ] {
            assert!(
                (allowable_bolt_shear(&input(bolt, surfaces)).unwrap() - expected).abs()
                    < tolerance
            );
        }
        let mut i = input(HighStrengthBolt::F8TGalvanized, 2);
        i.diameter_mm = 24;
        assert_eq!(allowable_bolt_shear(&i).unwrap(), 102_000.0);
    }
    #[test]
    fn each_end_uses_its_own_force_and_exact_boundary() {
        let i = input(HighStrengthBolt::F8TGalvanized, 1);
        let r = design_beam_bolts("b1", Some(&i), [70_800.0, -70_801.0], [true; 2], false);
        assert_eq!(r.ratios[0], Some(1.0));
        assert!(r.ratios[1].unwrap() > 1.0);
        assert!(r.complete());
        assert!(r.max_ratio().unwrap() > 1.0);
        let r = design_beam_bolts("b1", Some(&i), [70_800.0, 0.0], [true, false], true);
        assert_eq!(r.ratios, [Some(1.0), None]);
        assert!(!r.complete());
    }
    #[test]
    fn missing_and_invalid_conditions_never_become_a_zero_ratio() {
        let mut cases = Vec::new();
        let valid = input(HighStrengthBolt::F10T, 1);
        let mut i = valid.clone();
        i.count = 0;
        cases.push(i);
        let mut i = valid.clone();
        i.diameter_mm = 18;
        cases.push(i);
        let mut i = valid.clone();
        i.friction_surfaces = 3;
        cases.push(i);
        let mut i = valid.clone();
        i.surface_treatment_confirmed = false;
        cases.push(i);
        let mut i = valid.clone();
        i.concentric_shear_only_confirmed = false;
        cases.push(i);
        for i in cases.iter().map(Some).chain([None]) {
            let r = design_beam_bolts("b", i, [1.0, 2.0], [true; 2], false);
            assert!(!r.complete());
            assert!(!r.reasons.is_empty());
            assert_eq!(r.max_ratio(), None);
        }
        assert!(
            !design_beam_bolts("b", Some(&valid), [f64::NAN, 0.0], [true; 2], false).complete()
        );
    }
}
