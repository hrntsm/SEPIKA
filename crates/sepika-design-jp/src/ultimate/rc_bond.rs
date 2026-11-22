use sepika_core::units::ConcreteClass;

/// 大地震時の配筋区分。付着長さと不要断面までの距離 [mm] は付着検定断面から測る。
#[derive(Clone, Copy, Debug)]
pub enum RcUltimateBondBar {
    Through {
        clear_span_mm: f64,
        /// カットオフ筋併用時に、確認済みの曲げ分布に対応する距離を指定する。
        cutoff_unnecessary_distance_mm: Option<f64>,
        yielding: RcUltimateThroughBarYield,
    },
    Cutoff {
        development_length_mm: f64,
        clear_span_mm: f64,
        unnecessary_distance_mm: f64,
        stress: RcUltimateCutoffBarStress,
    },
}

/// 通し筋を持つ部材の曲げ降伏状態。
#[derive(Clone, Copy, Debug)]
pub enum RcUltimateThroughBarYield {
    BothEnds,
    OneEndWithOtherElastic,
    WithoutFlexuralYield {
        moment_distribution: RcUltimateBondMomentDistribution,
        stress: RcUltimateBondElasticStress,
    },
    Unverified,
}

/// 非降伏の通し筋で確認した地震時曲げモーメント分布。
#[derive(Clone, Copy, Debug)]
pub enum RcUltimateBondMomentDistribution {
    NearAntisymmetric,
    NearPinnedEnd,
}

/// 非降伏部材の付着検定断面で確認した鉄筋引張応力度 [N/mm²] と水平荷重時の割増。
#[derive(Clone, Copy, Debug)]
pub struct RcUltimateBondElasticStress {
    pub long_term_tensile_stress_mpa: f64,
    pub lateral_tensile_stress_mpa: f64,
    pub lateral_factor: f64,
}

/// カットオフ筋の設計用引張応力度の算定条件。応力度は [N/mm²]。
#[derive(Clone, Copy, Debug)]
pub enum RcUltimateCutoffBarStress {
    FlexuralYield,
    WithoutFlexuralYield(RcUltimateBondElasticStress),
    Unverified,
}

/// 付着照査を行う部材の曲げ軸。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RcUltimateBondAxis {
    Strong,
    Weak,
}

/// 打込み時に確認した鉄筋区分。上端筋は300 mm以上の打込みが下にある水平鉄筋。
#[derive(Clone, Copy, Debug)]
pub enum RcUltimateBondPlacement {
    TopCast,
    Other,
    Unverified,
}

/// 想定付着割裂面で確認したあき・かぶり [mm] と横補強筋。
#[derive(Clone, Copy, Debug)]
pub struct RcUltimateBondConfinement {
    pub clear_spacing_mm: f64,
    pub minimum_cover_mm: f64,
    pub splitting_plane_bar_count: u32,
    /// None は割裂面を横切る横補強筋がないことを確認した場合に指定する。
    pub transverse_rebar: Option<RcUltimateBondTransverseRebar>,
}

/// 想定付着割裂面を横切る一組の横補強筋の全断面積 [mm²] とその間隔 [mm]。
#[derive(Clone, Copy, Debug)]
pub struct RcUltimateBondTransverseRebar {
    pub area_mm2: f64,
    pub spacing_mm: f64,
}

/// 検討断面の付着照査を代表する一段配筋の組。
#[derive(Clone, Copy, Debug)]
pub enum RcUltimateBondDemand {
    /// 一組で必要な照査をすべて代表できる場合に指定する。
    Single(RcUltimateBondInput),
    /// 同じ検討断面・曲げ軸で、共通の曲げ降伏条件と不要断面までの距離を指定する。
    ThroughAndCutoff {
        through: RcUltimateBondInput,
        cutoff: RcUltimateBondInput,
    },
}

impl RcUltimateBondDemand {
    pub(super) fn inputs(self) -> ([Option<RcUltimateBondInput>; 2], Option<&'static str>) {
        match self {
            Self::Single(input) => {
                let incomplete = matches!(
                    input.bar,
                    RcUltimateBondBar::Through {
                        cutoff_unnecessary_distance_mm: Some(_),
                        ..
                    }
                );
                (
                    [Some(input), None],
                    incomplete.then_some("通し筋に併用するカットオフ筋の照査入力を確認できません"),
                )
            }
            Self::ThroughAndCutoff { through, cutoff } => {
                let issue = match (through.bar, cutoff.bar) {
                    (
                        RcUltimateBondBar::Through {
                            cutoff_unnecessary_distance_mm: Some(through_distance),
                            yielding,
                            ..
                        },
                        RcUltimateBondBar::Cutoff {
                            unnecessary_distance_mm: cutoff_distance,
                            stress,
                            ..
                        },
                    ) if through_distance == cutoff_distance && through.axis == cutoff.axis => {
                        let conflicting_yield = matches!(
                            (yielding, stress),
                            (
                                RcUltimateThroughBarYield::BothEnds
                                    | RcUltimateThroughBarYield::OneEndWithOtherElastic,
                                RcUltimateCutoffBarStress::WithoutFlexuralYield(_),
                            ) | (
                                RcUltimateThroughBarYield::WithoutFlexuralYield { .. },
                                RcUltimateCutoffBarStress::FlexuralYield,
                            )
                        );
                        conflicting_yield
                            .then_some("通し筋とカットオフ筋の曲げ降伏条件が一致しません")
                    }
                    _ => {
                        Some("通し筋とカットオフ筋の区分・検討軸・不要断面までの距離が一致しません")
                    }
                };
                ([Some(through), Some(cutoff)], issue)
            }
        }
    }
}

/// 検討対象を代表する異形鉄筋一組の付着照査入力。
/// 応力度と付着割裂の基準強度は [N/mm²]、呼び径と有効せいは [mm]。
#[derive(Clone, Copy, Debug)]
pub struct RcUltimateBondInput {
    pub bar: RcUltimateBondBar,
    pub axis: RcUltimateBondAxis,
    pub bar_designation_mm: f64,
    pub effective_depth_mm: f64,
    /// 確認済みの実降伏強度または強度上昇を見込んだ設計用降伏応力度 [N/mm²]。未確認は None。
    pub design_yield_stress_mpa: Option<f64>,
    pub placement: RcUltimateBondPlacement,
    /// None は割裂面の補強条件を確認できない場合に指定する。
    pub confinement: Option<RcUltimateBondConfinement>,
}

/// 大地震時の付着応力度 [N/mm²] と許容値 [N/mm²]。
#[derive(Clone, Copy, Debug)]
pub struct RcUltimateBondStress {
    pub demand_mpa: f64,
    pub capacity_mpa: f64,
}

/// 大地震時の付着応力度と独立したカットオフ筋の必要延長の照査。
#[derive(Clone, Copy, Debug)]
pub(super) struct RcUltimateBondCheck {
    pub(super) stress: Result<RcUltimateBondStress, &'static str>,
    pub(super) development_length_ok: Option<bool>,
}

impl RcUltimateBondInput {
    pub(super) fn clear_span_mm(self) -> f64 {
        match self.bar {
            RcUltimateBondBar::Through { clear_span_mm, .. }
            | RcUltimateBondBar::Cutoff { clear_span_mm, .. } => clear_span_mm,
        }
    }

    /// (16.5) または (16.6) の応力度と、確認できる場合の必要延長の適否を返す。
    pub(super) fn check(
        self,
        fc_mpa: f64,
        concrete_class: ConcreteClass,
    ) -> Result<RcUltimateBondCheck, &'static str> {
        let positive = |value: f64| value.is_finite() && value > 0.0;
        if !positive(self.effective_depth_mm) {
            return Err("終局付着の有効せいを確認できません");
        }
        let clear_span_mm = self.clear_span_mm();
        if !positive(clear_span_mm) {
            return Err("付着照査に必要な内法長さを確認できません");
        }
        let design_yield_stress_mpa = self
            .design_yield_stress_mpa
            .filter(|value| positive(*value))
            .ok_or("付着照査の実降伏強度または強度上昇を見込んだ設計用降伏応力度を確認できません");
        let (length, development_length_ok, tensile) = match self.bar {
            RcUltimateBondBar::Through {
                cutoff_unnecessary_distance_mm,
                yielding,
                ..
            } => {
                let cutoff_distance_mm = cutoff_unnecessary_distance_mm.unwrap_or(0.0);
                if !(0.0..clear_span_mm).contains(&cutoff_distance_mm) {
                    return Err("通し筋に併用するカットオフ筋の位置が適用範囲外です");
                }
                let tensile = match yielding {
                    RcUltimateThroughBarYield::BothEnds => {
                        design_yield_stress_mpa.map(|fy| 2.0 * fy)
                    }
                    RcUltimateThroughBarYield::OneEndWithOtherElastic => design_yield_stress_mpa,
                    RcUltimateThroughBarYield::WithoutFlexuralYield {
                        moment_distribution,
                        stress,
                    } => {
                        let coefficient = match moment_distribution {
                            RcUltimateBondMomentDistribution::NearAntisymmetric => 2.0,
                            RcUltimateBondMomentDistribution::NearPinnedEnd => 1.0,
                        };
                        design_yield_stress_mpa.and_then(|fy| {
                            stress
                                .limited_stress_mpa(fy)
                                .map(|sigma_d| coefficient * sigma_d)
                        })
                    }
                    RcUltimateThroughBarYield::Unverified => {
                        Err("通し筋の曲げ降伏状態を確認できません")
                    }
                };
                (clear_span_mm - cutoff_distance_mm, None, tensile)
            }
            RcUltimateBondBar::Cutoff {
                development_length_mm,
                clear_span_mm,
                unnecessary_distance_mm,
                stress,
            } => {
                if !(0.0..=clear_span_mm).contains(&unnecessary_distance_mm)
                    || !(0.0..=clear_span_mm).contains(&development_length_mm)
                {
                    return Err("カットオフ筋の付着長さが適用範囲外です");
                }
                (
                    development_length_mm,
                    Some(
                        development_length_mm - self.effective_depth_mm >= unnecessary_distance_mm,
                    ),
                    match stress {
                        RcUltimateCutoffBarStress::FlexuralYield => design_yield_stress_mpa,
                        RcUltimateCutoffBarStress::WithoutFlexuralYield(stress) => {
                            design_yield_stress_mpa.and_then(|fy| stress.limited_stress_mpa(fy))
                        }
                        RcUltimateCutoffBarStress::Unverified => {
                            Err("カットオフ筋の曲げ降伏状態を確認できません")
                        }
                    },
                )
            }
        };
        Ok(RcUltimateBondCheck {
            stress: self.check_stress(
                length - self.effective_depth_mm,
                tensile,
                fc_mpa,
                concrete_class,
            ),
            development_length_ok,
        })
    }

    fn check_stress(
        self,
        effective_length: f64,
        factored_tensile_stress_mpa: Result<f64, &'static str>,
        fc_mpa: f64,
        concrete_class: ConcreteClass,
    ) -> Result<RcUltimateBondStress, &'static str> {
        let positive = |value: f64| value.is_finite() && value > 0.0;
        if !positive(effective_length) {
            return Err("付着長さから有効せいを引いた長さを正の値として確認できません");
        }
        if !positive(self.bar_designation_mm) {
            return Err("終局付着の鉄筋・材料強度を確認できません");
        }
        let fb_mpa = self.placement.base_strength(fc_mpa, concrete_class)?;
        let confinement = self
            .confinement
            .ok_or("付着割裂面のあき・かぶり・横補強筋を確認できません")?
            .coefficient(self.bar_designation_mm)?;
        let demand_mpa =
            factored_tensile_stress_mpa? * (self.bar_designation_mm / effective_length) / 4.0;
        let capacity_mpa = confinement * fb_mpa;
        if !demand_mpa.is_finite() || demand_mpa < 0.0 || !positive(capacity_mpa) {
            return Err("終局付着の応力度または許容値を算定できません");
        }
        Ok(RcUltimateBondStress {
            demand_mpa,
            capacity_mpa,
        })
    }
}

impl RcUltimateBondPlacement {
    fn base_strength(
        self,
        fc_mpa: f64,
        concrete_class: ConcreteClass,
    ) -> Result<f64, &'static str> {
        if !fc_mpa.is_finite() || fc_mpa <= 0.0 {
            return Err("付着割裂の基準強度に必要なコンクリートのFcを確認できません");
        }
        let placement_factor = match self {
            Self::TopCast => 0.8,
            Self::Other => 1.0,
            Self::Unverified => return Err("コンクリート打込み時の上端筋区分を確認できません"),
        };
        let (concrete_factor, max_fc_mpa) = match concrete_class {
            ConcreteClass::Normal => (1.0, 60.0),
            ConcreteClass::Lightweight1 => (0.8, 36.0),
            ConcreteClass::Lightweight2 => (0.8, 27.0),
            ConcreteClass::UserDefined => {
                return Err("付着割裂の基準強度に必要なコンクリート区分を確認できません")
            }
        };
        if !(18.0..=max_fc_mpa).contains(&fc_mpa) {
            return Err("付着割裂の基準強度に用いるFcの適用範囲外です（普通18〜60・軽量1種18〜36・軽量2種18〜27 N/mm²）");
        }
        Ok((fc_mpa / 40.0 + 0.9) * placement_factor * concrete_factor)
    }
}

impl RcUltimateBondConfinement {
    fn coefficient(self, bar_designation_mm: f64) -> Result<f64, &'static str> {
        if self.splitting_plane_bar_count == 0
            || ![self.clear_spacing_mm, self.minimum_cover_mm]
                .into_iter()
                .all(|value| value.is_finite() && value >= 0.0)
        {
            return Err("付着割裂面の鉄筋本数・あき・かぶりを確認できません");
        }
        let c_ratio = (self.clear_spacing_mm / bar_designation_mm)
            .min(3.0 * self.minimum_cover_mm / bar_designation_mm)
            .min(5.0);
        let w_ratio = if let Some(rebar) = self.transverse_rebar {
            if ![rebar.area_mm2, rebar.spacing_mm]
                .into_iter()
                .all(|value| value.is_finite() && value > 0.0)
            {
                return Err("付着割裂面を横切る横補強筋の断面積・間隔を確認できません");
            }
            (80.0 * rebar.area_mm2
                / rebar.spacing_mm
                / f64::from(self.splitting_plane_bar_count)
                / bar_designation_mm)
                .min(2.5)
        } else {
            0.0
        };
        Ok((0.3 * (c_ratio + w_ratio) + 0.4).min(2.5))
    }
}

impl RcUltimateBondElasticStress {
    fn limited_stress_mpa(self, yield_stress_mpa: f64) -> Result<f64, &'static str> {
        let stress = self.long_term_tensile_stress_mpa
            + self.lateral_factor * self.lateral_tensile_stress_mpa;
        if ![
            self.long_term_tensile_stress_mpa,
            self.lateral_tensile_stress_mpa,
        ]
        .into_iter()
        .all(|value| value.is_finite() && value >= 0.0)
            || !self.lateral_factor.is_finite()
            || self.lateral_factor < 1.5
            || !stress.is_finite()
        {
            return Err("長期・水平荷重時の鉄筋応力度または割増を確認できません");
        }
        Ok(stress.min(yield_stress_mpa))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn through_and_cutoff_use_distinct_effective_bond_lengths() {
        let mut input = RcUltimateBondInput {
            bar: RcUltimateBondBar::Through {
                clear_span_mm: 2000.0,
                cutoff_unnecessary_distance_mm: None,
                yielding: RcUltimateThroughBarYield::OneEndWithOtherElastic,
            },
            axis: RcUltimateBondAxis::Strong,
            bar_designation_mm: 20.0,
            effective_depth_mm: 500.0,
            design_yield_stress_mpa: Some(400.0),
            placement: RcUltimateBondPlacement::Other,
            confinement: Some(RcUltimateBondConfinement {
                clear_spacing_mm: 20.0,
                minimum_cover_mm: 40.0,
                splitting_plane_bar_count: 2,
                transverse_rebar: Some(RcUltimateBondTransverseRebar {
                    area_mm2: 50.0,
                    spacing_mm: 100.0,
                }),
            }),
        };
        let check = |input: RcUltimateBondInput| input.check(24.0, ConcreteClass::Normal).unwrap();
        let through_only = check(input);
        assert_eq!(through_only.development_length_ok, None);
        let through_only = through_only.stress.unwrap();
        assert!((through_only.demand_mpa - 4.0 / 3.0).abs() < 1e-12);
        assert_eq!(through_only.capacity_mpa, 1.5);
        input.bar = RcUltimateBondBar::Through {
            clear_span_mm: 2000.0,
            cutoff_unnecessary_distance_mm: Some(500.0),
            yielding: RcUltimateThroughBarYield::BothEnds,
        };
        assert_eq!(check(input).stress.unwrap().demand_mpa, 4.0);
        input.bar = RcUltimateBondBar::Cutoff {
            development_length_mm: 900.0,
            clear_span_mm: 2000.0,
            unnecessary_distance_mm: 400.0,
            stress: RcUltimateCutoffBarStress::WithoutFlexuralYield(RcUltimateBondElasticStress {
                long_term_tensile_stress_mpa: 150.0,
                lateral_tensile_stress_mpa: 100.0,
                lateral_factor: 1.5,
            }),
        };
        let cutoff = check(input);
        assert_eq!(cutoff.development_length_ok, Some(true));
        assert_eq!(cutoff.stress.unwrap().demand_mpa, 3.75);
        input.bar = RcUltimateBondBar::Cutoff {
            development_length_mm: 899.0,
            clear_span_mm: 2000.0,
            unnecessary_distance_mm: 400.0,
            stress: RcUltimateCutoffBarStress::FlexuralYield,
        };
        assert_eq!(check(input).development_length_ok, Some(false));
        let confinement = input.confinement.as_mut().unwrap();
        confinement.clear_spacing_mm = 200.0;
        confinement.minimum_cover_mm = 80.0;
        confinement.transverse_rebar.as_mut().unwrap().area_mm2 = 150.0;
        assert_eq!(check(input).stress.unwrap().capacity_mpa, 3.75);
        input.confinement = None;
        assert!(check(input).stress.unwrap_err().contains("割裂面"));
    }
}
