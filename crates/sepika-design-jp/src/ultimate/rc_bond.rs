use sepika_core::units::ConcreteClass;
use sepika_solver::statics::analysis::SeismicDir;

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
    pub(super) fn has_cutoff(self) -> bool {
        matches!(
            self,
            Self::ThroughAndCutoff { .. }
                | Self::Single(RcUltimateBondInput {
                    bar: RcUltimateBondBar::Cutoff { .. }
                        | RcUltimateBondBar::Through {
                            cutoff_unnecessary_distance_mm: Some(_),
                            ..
                        },
                    ..
                })
        )
    }

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

/// 同じ水平載荷軸の正・負地震荷重ごとの非降伏RC梁の照査入力。
/// 内法で一定の矩形単独断面を持ち、1要素で表す梁に限る。
#[derive(Clone, Copy, Debug)]
pub struct RcUltimateBeamBondDemand {
    /// 地震荷重の全体座標系の軸。梁の局所曲げ軸・符号とは異なる。
    pub seismic_axis: SeismicDir,
    pub positive: Option<RcUltimateBeamBondDirectionDemand>,
    pub negative: Option<RcUltimateBeamBondDirectionDemand>,
}

/// 一つの地震荷重方向で曲げ非降伏を確認した矩形RC梁の付着照査入力。
#[derive(Clone, Copy, Debug)]
pub struct RcUltimateBeamBondDirectionDemand {
    pub section: RcUltimateBeamBondSection,
    /// この方向の必要な照査を指定断面・主筋で代表できることを別途確認した場合だけ true。
    pub represents_all_bond_checks: bool,
    /// 検定断面の採用主筋の組。応力度は同じ主筋の確認済み存在応力度、カットオフ筋の距離は
    /// 検定断面から対象の鉄筋端・不要断面が実在する側へ測った確認済みの値を指定する。
    pub bars: RcUltimateBondDemand,
}

/// 長期と割増水平荷重を合わせて最大絶対強軸曲げとなる、確認済みの付着検定断面。
/// 単軸曲げで採用主筋が引張となり、軸力・弱軸曲げで応力度が変わらない範囲を対象とする。
#[derive(Clone, Copy, Debug)]
pub struct RcUltimateBeamBondSection {
    /// i端内法面からの距離 [mm]。
    pub distance_from_i_face_mm: f64,
    /// 同断面の長期荷重時、水平荷重時それぞれの強軸曲げ [N·mm]。正は下端引張。
    pub long_term_mz_nmm: f64,
    pub lateral_mz_nmm: f64,
    /// 付着照査入力と存在応力度がともに対応する主筋。
    pub tension_side: RcUltimateBeamBondTensionSide,
}

/// RC梁の断面内で付着照査の対象とする上端または下端の主筋。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RcUltimateBeamBondTensionSide {
    Top,
    Bottom,
}

/// 正・負地震荷重それぞれの付着照査結果。
#[derive(Clone, Copy, Debug)]
pub struct RcUltimateBeamBondChecks {
    pub seismic_axis: SeismicDir,
    pub positive: RcUltimateBeamBondDirectionCheck,
    pub negative: RcUltimateBeamBondDirectionCheck,
}

/// 一つの地震荷重方向の結果。他方向の未算定にかかわらず保持する。
#[derive(Clone, Copy, Debug, Default)]
pub struct RcUltimateBeamBondDirectionCheck {
    /// 照査に指定された検定断面と主筋。入力なしは None。
    pub section: Option<RcUltimateBeamBondSection>,
    /// 通し筋の安全性検討用引張応力度 [N/mm²]。未算定または該当なしは None。
    pub through_sigma_d_mpa: Option<f64>,
    /// カットオフ筋の安全性検討用引張応力度 [N/mm²]。未算定または該当なしは None。
    pub cutoff_sigma_d_mpa: Option<f64>,
    /// 算定できた組のうち最小余裕度に対応する付着応力度と許容値。算定なしは None。
    pub stress: Option<RcUltimateBondStress>,
    /// 付着応力度とは独立した必要延長の判定。該当なしまたは未確認は None。
    pub development_length_ok: Option<bool>,
    /// 未算定の理由。確認済みの不適合がある場合も保持する。
    pub unchecked_reason: Option<&'static str>,
}

impl RcUltimateBeamBondDirectionCheck {
    pub(super) fn from_bars(
        bars: RcUltimateBondDemand,
        section: Option<RcUltimateBeamBondSection>,
        directional_conflict: Option<&'static str>,
        check_input: impl Fn(RcUltimateBondInput) -> Result<RcUltimateBondCheck, &'static str>,
    ) -> Self {
        let (inputs, grouping_issue) = bars.inputs();
        let factor_issue = match (section, inputs) {
            (Some(_), [Some(a), Some(b)]) => a
                .bar
                .elastic_stress()
                .zip(b.bar.elastic_stress())
                .filter(|(a, b)| a.lateral_factor != b.lateral_factor)
                .map(|_| "同じ地震荷重方向の通し筋とカットオフ筋で水平荷重の割増が一致しません"),
            _ => None,
        };
        let inconsistent_pair = (grouping_issue.is_some() || factor_issue.is_some())
            && matches!(bars, RcUltimateBondDemand::ThroughAndCutoff { .. });
        let mut result = Self {
            section,
            unchecked_reason: grouping_issue.or(factor_issue).or(directional_conflict),
            ..Self::default()
        };
        for input in inputs.into_iter().flatten() {
            let check = match check_input(input) {
                Ok(check) => check,
                Err(reason) => {
                    result.unchecked_reason.get_or_insert(reason);
                    continue;
                }
            };
            if let Some(length_ok) = check
                .development_length_ok
                .filter(|&ok| !inconsistent_pair || !ok)
            {
                result.development_length_ok =
                    Some(result.development_length_ok.unwrap_or(true) && length_ok);
            }
            if inconsistent_pair || directional_conflict.is_some() {
                continue;
            }
            match check.stress {
                Ok(stress) => {
                    if result
                        .stress
                        .is_none_or(|current| stress.margin() < current.margin())
                    {
                        result.stress = Some(stress);
                    }
                    match input.bar {
                        RcUltimateBondBar::Through { .. } => {
                            result.through_sigma_d_mpa = check.sigma_d_mpa
                        }
                        RcUltimateBondBar::Cutoff { .. } => {
                            result.cutoff_sigma_d_mpa = check.sigma_d_mpa
                        }
                    }
                }
                Err(reason) => {
                    result.unchecked_reason.get_or_insert(reason);
                }
            }
        }
        result
    }

    pub(super) fn detail(self, direction: &str) -> String {
        let mut detail = format!("{direction}地震荷重");
        if let Some(section) = self.section {
            let side = match section.tension_side {
                RcUltimateBeamBondTensionSide::Top => "上端主筋",
                RcUltimateBeamBondTensionSide::Bottom => "下端主筋",
            };
            detail.push_str(&format!(
                " i端内法面から {:.3} mm、{side}、ML={:.3} N·mm、ME={:.3} N·mm",
                section.distance_from_i_face_mm, section.long_term_mz_nmm, section.lateral_mz_nmm
            ));
        }
        for (bar, sigma_d) in [
            ("通し筋", self.through_sigma_d_mpa),
            ("カットオフ筋", self.cutoff_sigma_d_mpa),
        ] {
            if let Some(value) = sigma_d {
                detail.push_str(&format!("、{bar} σD={value:.3} N/mm²"));
            }
        }
        if let Some(reason) = self.unchecked_reason {
            detail.push_str(&format!("、付着応力度の未算定あり（{reason}）"));
        }
        if self.development_length_ok == Some(false) {
            detail.push_str("、カットオフ筋の必要延長不足");
        }
        detail
    }
}

impl RcUltimateBeamBondSection {
    pub(super) fn check_lengths(
        self,
        clear_span_mm: f64,
        bar: RcUltimateBondBar,
    ) -> Result<(), &'static str> {
        let x_mm = self.distance_from_i_face_mm;
        if !(0.0..=clear_span_mm).contains(&x_mm) {
            return Err("付着検定断面の位置が部材の内法区間にありません");
        }
        let max_distance_mm = x_mm.max(clear_span_mm - x_mm);
        let reachable = |distance_mm| (0.0..=max_distance_mm).contains(&distance_mm);
        match bar {
            RcUltimateBondBar::Cutoff { development_length_mm, .. }
                if !reachable(development_length_mm) =>
            {
                Err("カットオフ筋の付着長さが付着検定断面から到達できる内法区間を外れています")
            }
            RcUltimateBondBar::Cutoff { unnecessary_distance_mm, .. }
            | RcUltimateBondBar::Through {
                cutoff_unnecessary_distance_mm: Some(unnecessary_distance_mm),
                ..
            } if !reachable(unnecessary_distance_mm) => {
                Err("カットオフ筋の不要断面までの距離が付着検定断面から到達できる内法区間を外れています")
            }
            _ => Ok(()),
        }
    }

    pub(super) fn check_stress(
        self,
        stress: RcUltimateBondElasticStress,
        design_yield_stress_mpa: f64,
    ) -> Result<(), &'static str> {
        if stress.long_term_tensile_stress_mpa + stress.lateral_tensile_stress_mpa
            >= design_yield_stress_mpa
        {
            return Err("割増前の鉄筋応力度が設計用降伏応力度に達し、曲げ非降伏の条件と矛盾します");
        }
        let long = self.long_term_mz_nmm;
        let lateral = self.lateral_mz_nmm;
        let design = long + stress.lateral_factor * lateral;
        if ![long, lateral, design].into_iter().all(f64::is_finite) || design == 0.0 {
            return Err(
                "付着検定断面の長期・水平荷重時の強軸曲げまたは設計曲げの符号を確認できません",
            );
        }
        let side = if design < 0.0 {
            RcUltimateBeamBondTensionSide::Top
        } else {
            RcUltimateBeamBondTensionSide::Bottom
        };
        if self.tension_side != side {
            return Err("長期と割増水平荷重を合わせた曲げの引張側と採用主筋が一致しません");
        }
        if [long, lateral]
            .into_iter()
            .any(|m| m * design.signum() < 0.0)
        {
            return Err(
                "長期荷重または水平荷重で採用主筋が圧縮側となる場合の存在応力度は未算定です",
            );
        }
        if (long == 0.0) != (stress.long_term_tensile_stress_mpa == 0.0)
            || (lateral == 0.0) != (stress.lateral_tensile_stress_mpa == 0.0)
        {
            return Err("曲げと鉄筋応力度の一方のみがゼロで、単軸曲げの照査条件を確認できません");
        }
        Ok(())
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

impl RcUltimateBondStress {
    pub(super) fn margin(self) -> f64 {
        self.capacity_mpa / self.demand_mpa
    }
}

/// 大地震時の付着応力度と独立したカットオフ筋の必要延長の照査。
#[derive(Clone, Copy, Debug)]
pub(super) struct RcUltimateBondCheck {
    pub(super) stress: Result<RcUltimateBondStress, &'static str>,
    pub(super) development_length_ok: Option<bool>,
    pub(super) sigma_d_mpa: Option<f64>,
}

impl RcUltimateBondBar {
    pub(super) fn elastic_stress(self) -> Option<RcUltimateBondElasticStress> {
        match self {
            Self::Through {
                yielding: RcUltimateThroughBarYield::WithoutFlexuralYield { stress, .. },
                ..
            }
            | Self::Cutoff {
                stress: RcUltimateCutoffBarStress::WithoutFlexuralYield(stress),
                ..
            } => Some(stress),
            _ => None,
        }
    }
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
        let elastic_sigma_d_mpa = self
            .bar
            .elastic_stress()
            .map(|stress| design_yield_stress_mpa.and_then(|fy| stress.limited_stress_mpa(fy)));
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
                        ..
                    } => {
                        let coefficient = match moment_distribution {
                            RcUltimateBondMomentDistribution::NearAntisymmetric => 2.0,
                            RcUltimateBondMomentDistribution::NearPinnedEnd => 1.0,
                        };
                        elastic_sigma_d_mpa
                            .unwrap()
                            .map(|sigma_d| coefficient * sigma_d)
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
                        RcUltimateCutoffBarStress::WithoutFlexuralYield(_) => {
                            elastic_sigma_d_mpa.unwrap()
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
            sigma_d_mpa: elastic_sigma_d_mpa.and_then(Result::ok),
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
