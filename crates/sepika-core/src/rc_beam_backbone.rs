//! RC梁の方向別αy、三角形曲げ分布の総部材角と受動端ばね接続の診断。

use crate::error::CoreError;
use crate::rc_capacity::{rc_alpha_y_sugano_checked, rc_rebar_ratios, RcAlphaSection};

fn positive(name: &str, value: f64) -> Result<(), CoreError> {
    if value.is_finite() && value > 0.0 {
        Ok(())
    } else {
        Err(CoreError::InvalidInput(format!(
            "{name} は正の有限値が必要です"
        )))
    }
}

/// αyの数値評価と設計適用の診断。数式の有限値は適用範囲外でも保持する。
#[derive(Clone, Debug)]
pub struct RcAlphaEvaluation {
    pub alpha_y: f64,
    /// 原研究の入力資料域と正値の診断。Okも未確認原典や実験Validationの保証ではない。
    pub research_input_range: Result<(), CoreError>,
    /// 採用分岐の原典裏付けと対象実験Validationが未了のため、設計適用を保証しない。
    pub design_applicability: Result<(), CoreError>,
}

/// 寸法[mm]・引張筋[mm²]・材料比nから方向別αyを評価する。
/// 上端T形のみ、明示したコンクリート幾何IT[mm⁴]に対するI0/IT補正を適用する。
#[allow(clippy::too_many_arguments)]
pub fn rc_beam_alpha_evaluation(
    b_mm: f64,
    depth_mm: f64,
    effective_depth_mm: f64,
    tension_area_mm2: f64,
    section: RcAlphaSection,
    a_over_d: f64,
    n: f64,
    t_inertia_mm4: Option<f64>,
) -> Result<RcAlphaEvaluation, CoreError> {
    let ratios = rc_rebar_ratios(
        b_mm,
        depth_mm,
        effective_depth_mm,
        tension_area_mm2,
        section,
    )?;
    let raw = rc_alpha_y_sugano_checked(
        ratios.pt_alpha_ratio,
        a_over_d,
        effective_depth_mm / depth_mm,
        n,
    )?;
    let alpha_y = if matches!(section, RcAlphaSection::TTopTension { .. }) {
        let it = t_inertia_mm4.ok_or_else(|| {
            CoreError::InvalidInput("上端引張T形にはコンクリート幾何ITが必要です".into())
        })?;
        positive("IT", it)?;
        let i0 = b_mm * depth_mm.powi(3) / 12.0;
        if !i0.is_finite() || it < i0 {
            return Err(CoreError::InvalidInput(
                "ITは矩形梁I0以上の有限値が必要です".into(),
            ));
        }
        raw * i0 / it
    } else {
        raw
    };
    if !alpha_y.is_finite() {
        return Err(CoreError::InvalidInput("方向補正後のαyが非有限です".into()));
    }
    let research_input_range = if alpha_y <= 0.0 {
        Err(CoreError::InvalidInput(
            "αyが非正のため降伏骨格へ適用できません".into(),
        ))
    } else if !(0.004..=0.028).contains(&ratios.pt_alpha_ratio) || !(2.0..=5.0).contains(&a_over_d)
    {
        Err(CoreError::InvalidInput("原研究の資料域（pt=0.4–2.8%、a/D=2–5）外です。採用式の数値評価と設計適用を区別してください".into()))
    } else {
        Ok(())
    };
    Ok(RcAlphaEvaluation { alpha_y, research_input_range, design_applicability: Err(CoreError::InvalidInput("採用分岐の1987原典と対象実験Validationは未確認です。数式評価・資料域内を設計適用合格と扱いません".into())) })
}

/// 三角形曲げ分布の明示条件。逆対称形のLは剛域控除後の柔部材長[mm]。
#[derive(Clone, Copy, Debug)]
pub enum RcBeamMomentDiagram {
    /// 一定せん断・端から反曲点までの三角形分布。区間長[mm]内に反曲点が必要。
    Triangle {
        moment_nmm: f64,
        shear_n: f64,
        interval_mm: f64,
    },
    /// 逆対称で両半部材が同一であることを明示した基準分布。
    Antisymmetric {
        flexible_length_mm: f64,
        identical_halves: bool,
    },
    /// 分布荷重など、一定せん断・三角形曲げ分布を満たさない入力。
    Other,
}

impl RcBeamMomentDiagram {
    /// せん断スパンa[mm]。Q=0・反曲点区間外・非三角形は理由付きエラー。
    pub fn shear_span_mm(self) -> Result<f64, CoreError> {
        let a = match self {
            Self::Triangle {
                moment_nmm,
                shear_n,
                interval_mm,
            } => {
                positive("反曲点評価区間", interval_mm)?;
                if !moment_nmm.is_finite() || !shear_n.is_finite() || shear_n == 0.0 {
                    return Err(CoreError::InvalidInput(
                        "有限Mと非零有限Qが必要です。Q=0からa=L/2を推定しません".into(),
                    ));
                }
                let a = (moment_nmm / shear_n).abs();
                if a > interval_mm {
                    return Err(CoreError::InvalidInput(
                        "反曲点が評価区間外のため三角形基準へ接続できません".into(),
                    ));
                }
                a
            }
            Self::Antisymmetric {
                flexible_length_mm,
                identical_halves: true,
            } => {
                positive("柔部材長L", flexible_length_mm)?;
                flexible_length_mm / 2.0
            }
            Self::Antisymmetric {
                identical_halves: false,
                ..
            } => {
                return Err(CoreError::InvalidInput(
                    "逆対称基準には同一の両半部材が必要です".into(),
                ))
            }
            Self::Other => {
                return Err(CoreError::InvalidInput(
                    "一定せん断・三角形曲げ分布でないため自動接続できません".into(),
                ))
            }
        };
        positive("a=abs(M/Q)", a)?;
        Ok(a)
    }

    /// 総部材角に対するK0=3EcI/a[N·mm/rad]。EcI[N·mm²]は同じ基準断面を指定する。
    pub fn initial_stiffness(self, e_i_nmm2: f64) -> Result<f64, CoreError> {
        positive("EcI", e_i_nmm2)?;
        let k0 = 3.0 * e_i_nmm2 / self.shear_span_mm()?;
        positive("K0", k0)?;
        Ok(k0)
    }
}

/// 同一基準の総降伏部材角[rad]と追加端ばね割線柔性[rad/(N·mm)]。
#[derive(Clone, Debug)]
pub struct RcBeamYieldEvaluation {
    pub elastic_rotation_rad: f64,
    pub total_rotation_rad: f64,
    /// αy>1は負柔性のため接続不可。αy=1は追加柔性0でばね追加を不要とする。
    pub additional_flexibility: Result<f64, CoreError>,
}

/// My[N·mm]、正のαy、同一逆対称基準のS=6EcI/Lから総角と追加柔性を評価する。
/// 経験式の総角にせん断・抜出し成分を追加しない。αy>1でも総角の数値を保持する。
pub fn rc_beam_yield_evaluation(
    my_nmm: f64,
    alpha_y: f64,
    s_nmm: f64,
) -> Result<RcBeamYieldEvaluation, CoreError> {
    positive("My", my_nmm)?;
    positive("αy", alpha_y)?;
    positive("S", s_nmm)?;
    let elastic_rotation_rad = my_nmm / s_nmm;
    let total_rotation_rad = elastic_rotation_rad / alpha_y;
    positive("弾性部材角", elastic_rotation_rad)?;
    positive("総降伏部材角", total_rotation_rad)?;
    let additional_flexibility = if alpha_y > 1.0 {
        Err(CoreError::InvalidInput(
            "αy>1は負の追加柔性になるため受動端ばねへ接続できません".into(),
        ))
    } else {
        let flexibility = (1.0 / alpha_y - 1.0) / s_nmm;
        if flexibility.is_finite() {
            Ok(flexibility)
        } else {
            Err(CoreError::InvalidInput(
                "追加柔性が非有限のため接続できません".into(),
            ))
        }
    };
    Ok(RcBeamYieldEvaluation {
        elastic_rotation_rad,
        total_rotation_rad,
        additional_flexibility,
    })
}

/// 総骨格のひび割れ後区間勾配[N·mm/rad]。折れ点はMc<My、Rc<Ryが必要。
pub fn rc_beam_second_slope(
    mc_nmm: f64,
    my_nmm: f64,
    rc_rad: f64,
    ry_rad: f64,
) -> Result<f64, CoreError> {
    for (name, value) in [
        ("Mc", mc_nmm),
        ("My", my_nmm),
        ("Rc", rc_rad),
        ("Ry", ry_rad),
    ] {
        positive(name, value)?;
    }
    if mc_nmm >= my_nmm || rc_rad >= ry_rad {
        return Err(CoreError::InvalidInput(
            "骨格はMc<My・Rc<Ryが必要です".into(),
        ));
    }
    let slope = (my_nmm - mc_nmm) / (ry_rad - rc_rad);
    positive("第2勾配", slope)?;
    Ok(slope)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directional_t_alpha_uses_geometric_inertia_only_for_top() {
        let i0 = 5_400_000_000.0;
        // 梁300×600と上部150mmの有効幅1500mm。独立図心は412.5mm。
        let it = crate::section_shape::strip_section_properties(&[[450.0, 300.0], [150.0, 1500.0]])
            .unwrap()
            .inertia;
        assert_eq!(it, 10_293_750_000.0);
        let it = 2.0 * i0;
        let top = RcAlphaSection::TTopTension {
            effective_width_mm: 1500.0,
            slab_tension_area_mm2: 600.0,
        };
        let top_eval =
            rc_beam_alpha_evaluation(300.0, 600.0, 540.0, 1800.0, top, 3.0, 10.0, Some(it))
                .unwrap();
        assert!((top_eval.alpha_y - 0.130248).abs() < 1e-12);
        assert!((top_eval.alpha_y * 360e9 - 46_889_280_000.0).abs() < 1e-4);
        assert!(top_eval.research_input_range.is_ok());
        assert!(top_eval.design_applicability.is_err());
        assert!(
            rc_beam_alpha_evaluation(300.0, 600.0, 540.0, 1800.0, top, 3.0, 10.0, None).is_err()
        );
        let bottom = RcAlphaSection::TBottomTension {
            effective_width_mm: 1500.0,
        };
        let without_it =
            rc_beam_alpha_evaluation(300.0, 600.0, 540.0, 1800.0, bottom, 3.0, 10.0, None).unwrap();
        let with_it =
            rc_beam_alpha_evaluation(300.0, 600.0, 540.0, 1800.0, bottom, 3.0, 10.0, Some(it))
                .unwrap();
        assert!((without_it.alpha_y - 0.2782593).abs() < 1e-12);
        assert_eq!(with_it.alpha_y, without_it.alpha_y);
        assert!(with_it.research_input_range.is_err());
        let outside = rc_beam_alpha_evaluation(
            300.0,
            600.0,
            540.0,
            1800.0,
            RcAlphaSection::Rectangular,
            8.0,
            10.0,
            None,
        )
        .unwrap();
        assert!((outside.alpha_y - 0.7800705).abs() < 1e-12);
        assert!(outside.design_applicability.is_err());
        let nonpositive = rc_beam_alpha_evaluation(
            300.0,
            600.0,
            540.0,
            90000.0,
            RcAlphaSection::Rectangular,
            3.0,
            10.0,
            None,
        )
        .unwrap();
        assert!(nonpositive.alpha_y < 0.0);
        assert!(nonpositive.design_applicability.is_err());
    }

    #[test]
    fn triangle_requires_actual_span_and_never_substitutes_half_length() {
        let triangle = RcBeamMomentDiagram::Triangle {
            moment_nmm: -1e6,
            shear_n: 1000.0,
            interval_mm: 1500.0,
        };
        assert_eq!(triangle.shear_span_mm().unwrap(), 1000.0);
        assert_eq!(triangle.initial_stiffness(1e12).unwrap(), 3e9);
        for diagram in [
            RcBeamMomentDiagram::Triangle {
                moment_nmm: 1e6,
                shear_n: 0.0,
                interval_mm: 6000.0,
            },
            RcBeamMomentDiagram::Triangle {
                moment_nmm: 1e6,
                shear_n: 100.0,
                interval_mm: 6000.0,
            },
            RcBeamMomentDiagram::Triangle {
                moment_nmm: f64::NAN,
                shear_n: 100.0,
                interval_mm: 6000.0,
            },
            RcBeamMomentDiagram::Antisymmetric {
                flexible_length_mm: 6000.0,
                identical_halves: false,
            },
            RcBeamMomentDiagram::Other,
        ] {
            assert!(diagram.shear_span_mm().is_err());
        }
    }

    #[test]
    fn total_yield_rotation_and_passive_addition_are_separate() {
        let s = RcBeamMomentDiagram::Antisymmetric {
            flexible_length_mm: 6000.0,
            identical_halves: true,
        }
        .initial_stiffness(1e12)
        .unwrap();
        assert_eq!(s, 1e9);
        let result = rc_beam_yield_evaluation(1e6, 0.25, s).unwrap();
        assert_eq!(result.elastic_rotation_rad, 0.001);
        assert_eq!(result.total_rotation_rad, 0.004);
        assert!((1e6 * result.additional_flexibility.unwrap() - 0.003).abs() < 1e-15);
        let no_spring = rc_beam_yield_evaluation(1e6, 1.0, s).unwrap();
        assert_eq!(no_spring.additional_flexibility.unwrap(), 0.0);
        let negative = rc_beam_yield_evaluation(1e6, 2.0, s).unwrap();
        assert_eq!(negative.total_rotation_rad, 0.0005);
        assert!(negative
            .additional_flexibility
            .unwrap_err()
            .to_string()
            .contains("負"));
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(rc_beam_yield_evaluation(1e6, bad, s).is_err());
            assert!(rc_beam_yield_evaluation(1e6, 0.25, bad).is_err());
        }
        assert!(
            (rc_beam_second_slope(250000.0, 1e6, 0.00025, 0.004).unwrap() - 200000000.0).abs()
                < 1e-6
        );
    }
}
