#[path = "fixtures/rc_beam_asymmetric_model.rs"]
mod asymmetric;

use sepika_core::dof::Dof6Mask;
use sepika_core::model::Model;
use sepika_core::section_shape::{BeamStirrup, RebarSize, SectionShape, StandardRebar};
use sepika_design_jp::ultimate::{
    collect_rc_ultimate_checks, MemberDemand, RcUltimateBeamBondDemand,
    RcUltimateBeamBondDirectionDemand, RcUltimateBeamBondSection, RcUltimateBeamBondTensionSide,
    RcUltimateBeamHingeShearDemand, RcUltimateBeamHingeShearDirectionDemand, RcUltimateBondAxis,
    RcUltimateBondBar, RcUltimateBondConfinement, RcUltimateBondDemand,
    RcUltimateBondElasticStress, RcUltimateBondInput, RcUltimateBondMomentDistribution,
    RcUltimateBondPlacement, RcUltimateCutoffBarStress, RcUltimateThroughBarYield,
    StaticBeamHingeMechanism, UltimateCheck, UltimateShearOptions,
};
use sepika_solver::statics::analysis::SeismicDir;

fn model() -> Model {
    let mut model = asymmetric::model(false);
    model.ultimate_gravity_combination = Some(0);
    model.nodes[1].restraint = Dof6Mask::FIXED;
    model.elements[0].rigid_zone.face_i = Some(0.0);
    model.elements[0].rigid_zone.face_j = Some(0.0);
    let mut section = asymmetric::section(270.0, 350.0, 19.0, &[4], &[2]);
    let Some(SectionShape::RcBeamRect { rebar, .. }) = section.shape.as_mut() else {
        unreachable!()
    };
    rebar.main_dia = RebarSize::Standard(StandardRebar::D19);
    rebar.cover = 50.0;
    rebar.stirrup = BeamStirrup {
        dia: RebarSize::DiameterMm(0.0),
        pitch: 0.0,
        legs: 0,
    };
    section.shear_rebar_material = None;
    model.sections[0] = section;
    model.validate().unwrap();
    model
}

fn input(bar: RcUltimateBondBar, side: RcUltimateBeamBondTensionSide) -> RcUltimateBondInput {
    let top = side == RcUltimateBeamBondTensionSide::Top;
    RcUltimateBondInput {
        bar,
        axis: RcUltimateBondAxis::Strong,
        bar_designation_mm: 19.0,
        effective_depth_mm: 290.5,
        design_yield_stress_mpa: Some(380.0),
        placement: RcUltimateBondPlacement::Other,
        confinement: Some(RcUltimateBondConfinement {
            clear_spacing_mm: if top { 31.233333333333334 } else { 131.9 },
            minimum_cover_mm: 49.95,
            splitting_plane_bar_count: if top { 4 } else { 2 },
            transverse_rebar: None,
        }),
    }
}

fn through(
    stress: RcUltimateBondElasticStress,
    side: RcUltimateBeamBondTensionSide,
) -> RcUltimateBondDemand {
    RcUltimateBondDemand::Single(input(
        RcUltimateBondBar::Through {
            clear_span_mm: 3000.0,
            cutoff_unnecessary_distance_mm: None,
            yielding: RcUltimateThroughBarYield::WithoutFlexuralYield {
                moment_distribution: RcUltimateBondMomentDistribution::NearAntisymmetric,
                stress,
            },
        },
        side,
    ))
}

fn top_stress() -> RcUltimateBondElasticStress {
    RcUltimateBondElasticStress {
        long_term_tensile_stress_mpa: 42.85468630782416,
        lateral_tensile_stress_mpa: 89.28059647463365,
        lateral_factor: 1.5,
    }
}

fn demand() -> RcUltimateBeamBondDemand {
    use RcUltimateBeamBondTensionSide::{Bottom, Top};
    RcUltimateBeamBondDemand {
        seismic_axis: SeismicDir::X,
        positive: Some(RcUltimateBeamBondDirectionDemand {
            section: RcUltimateBeamBondSection {
                distance_from_i_face_mm: 0.0,
                long_term_mz_nmm: -12e6,
                lateral_mz_nmm: -25e6,
                tension_side: Top,
            },
            represents_all_bond_checks: true,
            bars: through(top_stress(), Top),
        }),
        negative: Some(RcUltimateBeamBondDirectionDemand {
            section: RcUltimateBeamBondSection {
                distance_from_i_face_mm: 3000.0,
                long_term_mz_nmm: 10e6,
                lateral_mz_nmm: 30e6,
                tension_side: Bottom,
            },
            represents_all_bond_checks: true,
            bars: through(
                RcUltimateBondElasticStress {
                    long_term_tensile_stress_mpa: 69.6522970525439,
                    lateral_tensile_stress_mpa: 208.9568911576317,
                    lateral_factor: 2.0,
                },
                Bottom,
            ),
        }),
    }
}

fn check(model: &Model, demand: MemberDemand) -> UltimateCheck {
    let [result] = collect_rc_ultimate_checks(
        model,
        &[(model.elements[0].id, demand)],
        &UltimateShearOptions::default(),
    )
    .unwrap()
    .try_into()
    .expect("RC梁1本の終局検定結果");
    result
}

fn total() -> MemberDemand {
    MemberDemand {
        min_axial_for_shear_n: Some(0.0),
        ..MemberDemand::from_pushover(0.0, 49.5e6, 0.0, 10_000.0, 0.0, 0.0)
    }
}

fn directional(model: &Model, bond: RcUltimateBeamBondDemand) -> UltimateCheck {
    check(
        model,
        MemberDemand {
            beam_bond: Some(bond),
            ..total()
        },
    )
}

#[test]
fn confirmed_load_directions_use_distinct_sections_and_tension_layers() {
    let checked = directional(&model(), demand());
    let by_direction = checked.beam_bond.unwrap();
    assert_eq!(by_direction.seismic_axis, SeismicDir::X);
    assert_eq!(by_direction.positive.unchecked_reason, None);
    assert_eq!(by_direction.negative.unchecked_reason, None);
    let top = by_direction.positive.section.unwrap();
    let bottom = by_direction.negative.section.unwrap();
    assert_eq!(
        (top.distance_from_i_face_mm, top.tension_side),
        (0.0, RcUltimateBeamBondTensionSide::Top)
    );
    assert_eq!(
        (bottom.distance_from_i_face_mm, bottom.tension_side),
        (3000.0, RcUltimateBeamBondTensionSide::Bottom)
    );
    assert_eq!((top.long_term_mz_nmm, top.lateral_mz_nmm), (-12e6, -25e6));
    assert_eq!(
        (bottom.long_term_mz_nmm, bottom.lateral_mz_nmm),
        (10e6, 30e6)
    );

    // 付着入力とは独立の換算断面計算による存在応力度。上端176.775...、下端487.566...を380で制限。
    let positive_sigma = by_direction.positive.through_sigma_d_mpa.unwrap();
    assert!((positive_sigma - 176.77558101977465).abs() < 1e-10);
    assert_eq!(by_direction.negative.through_sigma_d_mpa, Some(380.0));
    let top_bond = by_direction.positive.stress.unwrap();
    let bottom_bond = by_direction.negative.stress.unwrap();
    for (bond, tau, kfb, margin) in [
        (
            top_bond,
            0.6198073517947441,
            1.3397368421052631,
            2.1615375135933066,
        ),
        (bottom_bond, 1.332349141908101, 2.85, 2.139078947368421),
    ] {
        assert!((bond.demand_mpa - tau).abs() < 1e-12);
        assert!((bond.capacity_mpa - kfb).abs() < 1e-12);
        assert!((bond.capacity_mpa / bond.demand_mpa - margin).abs() < 1e-12);
        assert!(bond.demand_mpa < bond.capacity_mpa);
    }
    assert_eq!(
        checked.bond_stress.unwrap().demand_mpa,
        bottom_bond.demand_mpa
    );
    assert!((checked.bond_margin - 2.139078947368421).abs() < 1e-12);
    assert!(checked.bond_margin > 1.0);
    assert!(checked.bond_cutoff_absent && checked.bond_unchecked_reason.is_none());
    assert!(!checked.ok && checked.qsu.is_nan());
    assert!(checked
        .unchecked_reason
        .unwrap()
        .contains("せん断補強筋を確認できません"));
}

#[test]
fn invalid_directional_stress_is_not_replaced_by_opposite_bars_or_zero() {
    use RcUltimateBeamBondTensionSide::Top;
    let model = model();
    let original = demand().positive.unwrap();
    let top = original.section;
    for (section, stress, reason) in [
        (
            RcUltimateBeamBondSection {
                long_term_mz_nmm: 100e6,
                ..top
            },
            top_stress(),
            "引張側と採用主筋が一致しません",
        ),
        (
            RcUltimateBeamBondSection {
                long_term_mz_nmm: 10e6,
                ..top
            },
            top_stress(),
            "採用主筋が圧縮側",
        ),
        (
            top,
            RcUltimateBondElasticStress {
                lateral_tensile_stress_mpa: 0.0,
                ..top_stress()
            },
            "曲げと鉄筋応力度の一方のみがゼロ",
        ),
        (
            RcUltimateBeamBondSection {
                long_term_mz_nmm: 0.0,
                ..top
            },
            top_stress(),
            "曲げと鉄筋応力度の一方のみがゼロ",
        ),
        (
            top,
            RcUltimateBondElasticStress {
                lateral_factor: 1.49,
                ..top_stress()
            },
            "鉄筋応力度または割増",
        ),
        (
            top,
            RcUltimateBondElasticStress {
                lateral_tensile_stress_mpa: f64::NAN,
                ..top_stress()
            },
            "鉄筋応力度または割増",
        ),
        (
            RcUltimateBeamBondSection {
                distance_from_i_face_mm: 3001.0,
                ..top
            },
            top_stress(),
            "付着検定断面の位置",
        ),
    ] {
        let checked = directional(
            &model,
            RcUltimateBeamBondDemand {
                positive: Some(RcUltimateBeamBondDirectionDemand {
                    section,
                    bars: through(stress, Top),
                    ..original
                }),
                ..demand()
            },
        );
        let by_direction = checked.beam_bond.unwrap();
        assert!(!checked.ok, "{reason}: {checked:?}");
        assert!(by_direction.positive.through_sigma_d_mpa.is_none());
        assert!(by_direction.positive.stress.is_none());
        assert!(
            by_direction
                .positive
                .unchecked_reason
                .unwrap()
                .contains(reason),
            "{checked:?}"
        );
        assert_eq!(by_direction.negative.through_sigma_d_mpa, Some(380.0));
    }
    let without_gravity_bending = directional(
        &model,
        RcUltimateBeamBondDemand {
            positive: Some(RcUltimateBeamBondDirectionDemand {
                section: RcUltimateBeamBondSection {
                    long_term_mz_nmm: 0.0,
                    ..top
                },
                bars: through(
                    RcUltimateBondElasticStress {
                        long_term_tensile_stress_mpa: 0.0,
                        ..top_stress()
                    },
                    Top,
                ),
                ..original
            }),
            ..demand()
        },
    );
    let positive = without_gravity_bending.beam_bond.unwrap().positive;
    assert!(positive.unchecked_reason.is_none());
    assert!((positive.through_sigma_d_mpa.unwrap() - 133.9208947119505).abs() < 1e-10);
}

#[test]
fn missing_direction_or_unconfirmed_coverage_does_not_pass_and_preserves_ng() {
    use RcUltimateBeamBondTensionSide::Top;
    let model = model();
    let supplied = demand();
    let positive = supplied.positive.unwrap();
    let partial = directional(
        &model,
        RcUltimateBeamBondDemand {
            negative: None,
            ..supplied
        },
    );
    assert!(!partial.ok && partial.unchecked_reason.is_some());
    assert!(partial
        .beam_bond
        .unwrap()
        .negative
        .unchecked_reason
        .is_some());
    assert!(partial.bond_margin > 1.0);
    assert!(!partial.bond_cutoff_absent);
    assert!(partial
        .bond_unchecked_reason
        .as_deref()
        .is_some_and(|reason| reason.contains("存在応力度を確認できません")));

    let not_representative = directional(
        &model,
        RcUltimateBeamBondDemand {
            positive: Some(RcUltimateBeamBondDirectionDemand {
                represents_all_bond_checks: false,
                ..positive
            }),
            ..supplied
        },
    );
    assert!(!not_representative.ok && not_representative.bond_margin > 1.0);
    assert!(!not_representative.bond_cutoff_absent);
    assert!(not_representative
        .beam_bond
        .unwrap()
        .positive
        .unchecked_reason
        .unwrap()
        .contains("必要な付着照査を代表"));

    let legacy = check(
        &model,
        MemberDemand {
            bond: Some(positive.bars),
            ..total()
        },
    );
    assert!(legacy.beam_bond.is_none() && legacy.bond_stress.is_none());
    assert!(legacy
        .unchecked_reason
        .unwrap()
        .contains("正負地震荷重ごとの検定断面"));

    for (ld, factor, tau_available, extension_ok) in
        [(700.0, 1.5, true, true), (310.0, 1.49, false, false)]
    {
        let checked = directional(
            &model,
            RcUltimateBeamBondDemand {
                positive: Some(RcUltimateBeamBondDirectionDemand {
                    represents_all_bond_checks: false,
                    bars: RcUltimateBondDemand::Single(input(
                        RcUltimateBondBar::Cutoff {
                            development_length_mm: ld,
                            clear_span_mm: 3000.0,
                            unnecessary_distance_mm: 200.0,
                            stress: RcUltimateCutoffBarStress::WithoutFlexuralYield(
                                RcUltimateBondElasticStress {
                                    lateral_factor: factor,
                                    ..top_stress()
                                },
                            ),
                        },
                        Top,
                    )),
                    ..positive
                }),
                negative: None,
                ..supplied
            },
        );
        let result = checked.beam_bond.unwrap().positive;
        assert!(
            !checked.ok && checked.unchecked_reason.is_none(),
            "{checked:?}"
        );
        assert_eq!(result.development_length_ok, Some(extension_ok));
        assert_eq!(
            checked.bond_development_length_ok,
            (!extension_ok).then_some(false)
        );
        assert_eq!(result.stress.is_some(), tau_available);
        assert!(checked.bond_unchecked_reason.is_some() && !checked.bond_cutoff_absent);
        if tau_available {
            assert!((result.cutoff_sigma_d_mpa.unwrap() - 176.77558101977465).abs() < 1e-10);
            assert!(checked.bond_margin < 1.0);
            assert!(result
                .unchecked_reason
                .unwrap()
                .contains("必要な付着照査を代表"));
        } else {
            assert!(result.cutoff_sigma_d_mpa.is_none());
            assert!(checked.detail.contains("カットオフ筋の必要延長不足"));
        }
    }
}

#[test]
fn cutoff_absence_and_length_pass_require_valid_through_only_geometry_in_every_direction() {
    use RcUltimateBeamBondTensionSide::Top;
    let model = model();
    let supplied = demand();
    let positive = supplied.positive.unwrap();
    let negative = supplied.negative.unwrap();
    let RcUltimateBondDemand::Single(negative_bar) = negative.bars else {
        unreachable!()
    };
    let negative_unknown_stress = RcUltimateBeamBondDirectionDemand {
        bars: RcUltimateBondDemand::Single(RcUltimateBondInput {
            confinement: None,
            ..negative_bar
        }),
        ..negative
    };
    let partial = directional(
        &model,
        RcUltimateBeamBondDemand {
            negative: Some(negative_unknown_stress),
            ..supplied
        },
    );
    assert!(partial.bond_stress.is_some() && partial.bond_unchecked_reason.is_some());
    assert!(partial.bond_cutoff_absent);
    let partial = directional(
        &model,
        RcUltimateBeamBondDemand {
            positive: Some(RcUltimateBeamBondDirectionDemand {
                bars: through(
                    RcUltimateBondElasticStress {
                        long_term_tensile_stress_mpa: f64::NAN,
                        ..top_stress()
                    },
                    Top,
                ),
                ..positive
            }),
            ..supplied
        },
    );
    assert!(partial.bond_stress.is_some() && partial.bond_unchecked_reason.is_some());
    assert!(partial.bond_cutoff_absent);

    for (invalid_direction, reason) in [
        (
            RcUltimateBeamBondDirectionDemand {
                section: RcUltimateBeamBondSection {
                    distance_from_i_face_mm: 3001.0,
                    ..negative.section
                },
                ..negative
            },
            "検定断面の位置が部材の内法区間にありません",
        ),
        (
            RcUltimateBeamBondDirectionDemand {
                bars: RcUltimateBondDemand::Single(RcUltimateBondInput {
                    axis: RcUltimateBondAxis::Weak,
                    ..negative_bar
                }),
                ..negative
            },
            "強軸方向に限ります",
        ),
        (
            RcUltimateBeamBondDirectionDemand {
                section: RcUltimateBeamBondSection {
                    long_term_mz_nmm: -100e6,
                    ..negative.section
                },
                ..negative
            },
            "引張側と採用主筋が一致しません",
        ),
    ] {
        let checked = directional(
            &model,
            RcUltimateBeamBondDemand {
                negative: Some(invalid_direction),
                ..supplied
            },
        );
        assert!(!checked.bond_cutoff_absent && checked.bond_development_length_ok.is_none());
        assert!(checked
            .bond_unchecked_reason
            .as_deref()
            .unwrap()
            .contains(reason));

        for (length_mm, ok) in [(1600.0, true), (310.0, false)] {
            let checked = directional(
                &model,
                RcUltimateBeamBondDemand {
                    positive: Some(RcUltimateBeamBondDirectionDemand {
                        bars: RcUltimateBondDemand::Single(input(
                            RcUltimateBondBar::Cutoff {
                                development_length_mm: length_mm,
                                clear_span_mm: 3000.0,
                                unnecessary_distance_mm: 200.0,
                                stress: RcUltimateCutoffBarStress::WithoutFlexuralYield(
                                    top_stress(),
                                ),
                            },
                            Top,
                        )),
                        ..positive
                    }),
                    negative: Some(invalid_direction),
                    ..supplied
                },
            );
            assert_eq!(
                checked.beam_bond.unwrap().positive.development_length_ok,
                Some(ok)
            );
            assert_eq!(checked.bond_development_length_ok, (!ok).then_some(false));
            assert!(!checked.bond_cutoff_absent);
        }
    }
}

#[test]
fn cutoff_length_pass_requires_the_design_tension_side_but_not_a_calculated_stress() {
    use RcUltimateBeamBondTensionSide::{Bottom, Top};
    let model = model();
    let supplied = demand();
    let positive = supplied.positive.unwrap();
    let RcUltimateBondDemand::Single(positive_bar) = positive.bars else {
        unreachable!()
    };
    let confinement = positive_bar.confinement;

    for (side, length_mm, stresses, confinement, expected) in [
        (Top, 1600.0, top_stress(), None, Some(true)),
        (
            Top,
            1600.0,
            RcUltimateBondElasticStress {
                long_term_tensile_stress_mpa: f64::NAN,
                ..top_stress()
            },
            confinement,
            Some(true),
        ),
        (Bottom, 1600.0, top_stress(), confinement, None),
        (Bottom, 1600.0, top_stress(), None, None),
        (Bottom, 310.0, top_stress(), confinement, Some(false)),
    ] {
        let bar = RcUltimateBondBar::Cutoff {
            development_length_mm: length_mm,
            clear_span_mm: 3000.0,
            unnecessary_distance_mm: 200.0,
            stress: RcUltimateCutoffBarStress::WithoutFlexuralYield(stresses),
        };
        let checked = directional(
            &model,
            RcUltimateBeamBondDemand {
                positive: Some(RcUltimateBeamBondDirectionDemand {
                    section: RcUltimateBeamBondSection {
                        tension_side: side,
                        ..positive.section
                    },
                    bars: RcUltimateBondDemand::Single(RcUltimateBondInput {
                        confinement,
                        ..input(bar, side)
                    }),
                    ..positive
                }),
                ..supplied
            },
        );
        let positive = checked.beam_bond.unwrap().positive;
        assert!(
            positive.stress.is_none() && positive.unchecked_reason.is_some(),
            "{checked:?}"
        );
        assert_eq!(checked.bond_development_length_ok, expected, "{checked:?}");
        assert!(!checked.bond_cutoff_absent && !checked.ok, "{checked:?}");
        assert_eq!(
            checked.unchecked_reason.is_none(),
            expected == Some(false),
            "{checked:?}"
        );
        if expected.is_none() {
            assert!(checked
                .detail
                .contains("引張側の一致を確認できないため、必要延長は未算定"));
        }
        if confinement.is_none() {
            assert!(positive
                .unchecked_reason
                .unwrap()
                .contains("横補強筋を確認できません"));
        }
    }
}

#[test]
fn paired_cutoff_can_confirm_its_length_without_using_the_through_bar_stress() {
    use RcUltimateBeamBondTensionSide::Top;
    let model = model();
    let supplied = demand();
    let positive = supplied.positive.unwrap();
    let through = input(
        RcUltimateBondBar::Through {
            clear_span_mm: 3000.0,
            cutoff_unnecessary_distance_mm: Some(200.0),
            yielding: RcUltimateThroughBarYield::WithoutFlexuralYield {
                moment_distribution: RcUltimateBondMomentDistribution::NearAntisymmetric,
                stress: top_stress(),
            },
        },
        Top,
    );
    let cutoff = |development_length_mm, distance_mm, stress| {
        input(
            RcUltimateBondBar::Cutoff {
                development_length_mm,
                clear_span_mm: 3000.0,
                unnecessary_distance_mm: distance_mm,
                stress,
            },
            Top,
        )
    };
    let unknown = cutoff(1600.0, 200.0, RcUltimateCutoffBarStress::Unverified);
    let known = RcUltimateCutoffBarStress::WithoutFlexuralYield(top_stress());
    let invalid_geometry = RcUltimateBondInput {
        effective_depth_mm: 291.0,
        ..through
    };
    let through_with_factor = |lateral_factor| RcUltimateBondInput {
        bar: RcUltimateBondBar::Through {
            clear_span_mm: 3000.0,
            cutoff_unnecessary_distance_mm: Some(200.0),
            yielding: RcUltimateThroughBarYield::WithoutFlexuralYield {
                moment_distribution: RcUltimateBondMomentDistribution::NearAntisymmetric,
                stress: RcUltimateBondElasticStress {
                    lateral_factor,
                    ..top_stress()
                },
            },
        },
        ..through
    };
    for (through, cutoff, expected, reason, through_stress_calculated) in [
        (
            through,
            unknown,
            Some(true),
            "カットオフ筋の曲げ降伏状態",
            true,
        ),
        (
            RcUltimateBondInput {
                confinement: None,
                ..through
            },
            unknown,
            Some(true),
            "横補強筋を確認できません",
            false,
        ),
        (
            through_with_factor(1.49),
            unknown,
            Some(true),
            "鉄筋応力度または割増",
            false,
        ),
        (invalid_geometry, unknown, None, "有効せいが検討方向", false),
        (
            invalid_geometry,
            cutoff(1600.0, 200.0, known),
            None,
            "有効せいが検討方向",
            false,
        ),
        (
            invalid_geometry,
            cutoff(310.0, 200.0, known),
            Some(false),
            "有効せいが検討方向",
            false,
        ),
        (
            through,
            cutoff(1600.0, 250.0, RcUltimateCutoffBarStress::Unverified),
            None,
            "距離が一致しません",
            false,
        ),
        (
            through,
            cutoff(
                1600.0,
                200.0,
                RcUltimateCutoffBarStress::WithoutFlexuralYield(RcUltimateBondElasticStress {
                    lateral_factor: 2.0,
                    ..top_stress()
                }),
            ),
            None,
            "水平荷重の割増が一致しません",
            false,
        ),
    ] {
        let checked = directional(
            &model,
            RcUltimateBeamBondDemand {
                positive: Some(RcUltimateBeamBondDirectionDemand {
                    bars: RcUltimateBondDemand::ThroughAndCutoff { through, cutoff },
                    ..positive
                }),
                ..supplied
            },
        );
        let positive = checked.beam_bond.unwrap().positive;
        assert_eq!(checked.bond_development_length_ok, expected, "{checked:?}");
        if matches!(
            cutoff.bar,
            RcUltimateBondBar::Cutoff {
                stress: RcUltimateCutoffBarStress::Unverified,
                ..
            }
        ) {
            assert!(positive.cutoff_sigma_d_mpa.is_none());
        }
        assert!(
            positive.unchecked_reason.unwrap().contains(reason),
            "{checked:?}"
        );
        assert_eq!(
            positive.through_sigma_d_mpa.is_some(),
            through_stress_calculated
        );
    }

    for (ld_mm, expected) in [(1600.0, true), (310.0, false)] {
        let checked = directional(
            &model,
            RcUltimateBeamBondDemand {
                positive: Some(RcUltimateBeamBondDirectionDemand {
                    bars: RcUltimateBondDemand::ThroughAndCutoff {
                        through: through_with_factor(f64::NAN),
                        cutoff: cutoff(
                            ld_mm,
                            200.0,
                            RcUltimateCutoffBarStress::WithoutFlexuralYield(
                                RcUltimateBondElasticStress {
                                    lateral_factor: f64::NAN,
                                    ..top_stress()
                                },
                            ),
                        ),
                    },
                    ..positive
                }),
                ..supplied
            },
        );
        let positive = checked.beam_bond.unwrap().positive;
        assert_eq!(
            positive.development_length_ok,
            Some(expected),
            "{checked:?}"
        );
        assert_eq!(
            checked.bond_development_length_ok,
            Some(expected),
            "{checked:?}"
        );
        assert!(
            positive.stress.is_none()
                && positive.through_sigma_d_mpa.is_none()
                && positive.cutoff_sigma_d_mpa.is_none(),
            "{checked:?}"
        );
        assert!(positive
            .unchecked_reason
            .unwrap()
            .contains("鉄筋応力度または割増"));
    }

    let confirmed_cutoff = cutoff(1600.0, 200.0, known);
    let unconfirmed_depth = RcUltimateBondInput {
        effective_depth_mm: 291.0,
        ..confirmed_cutoff
    };
    for (represents_all_bond_checks, tension_side, through_span, cutoff, local_length, detail) in [
        (
            false,
            Top,
            3000.0,
            confirmed_cutoff,
            Some(true),
            "必要延長の全体判定に必要な、この方向の指定断面・主筋の代表性",
        ),
        (
            true,
            RcUltimateBeamBondTensionSide::Bottom,
            3000.0,
            confirmed_cutoff,
            None,
            "引張側の一致を確認できないため、必要延長",
        ),
        (
            true,
            Top,
            3000.0,
            unconfirmed_depth,
            None,
            "付着照査の有効せいが検討方向の配筋と一致しません",
        ),
        (
            true,
            Top,
            3000.0,
            cutoff(3001.0, 200.0, known),
            None,
            "カットオフ筋の付着長さが付着検定断面から到達できる内法区間を外れています",
        ),
        (
            true,
            Top,
            2999.0,
            unconfirmed_depth,
            None,
            "付着照査の有効せいが検討方向の配筋と一致しません",
        ),
    ] {
        let mut through = RcUltimateBondInput {
            confinement: None,
            ..through
        };
        let RcUltimateBondBar::Through { clear_span_mm, .. } = &mut through.bar else {
            unreachable!()
        };
        *clear_span_mm = through_span;
        let checked = directional(
            &model,
            RcUltimateBeamBondDemand {
                positive: Some(RcUltimateBeamBondDirectionDemand {
                    represents_all_bond_checks,
                    section: RcUltimateBeamBondSection {
                        tension_side,
                        ..positive.section
                    },
                    bars: RcUltimateBondDemand::ThroughAndCutoff { through, cutoff },
                }),
                ..supplied
            },
        );
        let positive = checked.beam_bond.unwrap().positive;
        assert_eq!(positive.development_length_ok, local_length, "{checked:?}");
        assert_eq!(checked.bond_development_length_ok, None, "{checked:?}");
        let stress_reason = if through_span == 3000.0 {
            "横補強筋を確認できません"
        } else {
            "付着照査の内法長さが部材の内法長さと一致しません"
        };
        assert!(positive.unchecked_reason.unwrap().contains(stress_reason));
        assert!(checked.detail.contains(detail), "{checked:?}");
    }
}

#[test]
fn single_bar_in_the_tension_layer_cannot_confirm_two_distinct_bond_groups() {
    use RcUltimateBeamBondTensionSide::Top;
    let mut model = model();
    let Some(SectionShape::RcBeamRect { rebar, .. }) = model.sections[0].shape.as_mut() else {
        unreachable!()
    };
    rebar.top = vec![1];
    model.validate().unwrap();
    let supplied = demand();
    let positive = supplied.positive.unwrap();
    for (yielding, stress) in [
        (
            RcUltimateThroughBarYield::Unverified,
            RcUltimateCutoffBarStress::Unverified,
        ),
        (
            RcUltimateThroughBarYield::WithoutFlexuralYield {
                moment_distribution: RcUltimateBondMomentDistribution::NearAntisymmetric,
                stress: top_stress(),
            },
            RcUltimateCutoffBarStress::WithoutFlexuralYield(top_stress()),
        ),
    ] {
        let through = input(
            RcUltimateBondBar::Through {
                clear_span_mm: 3000.0,
                cutoff_unnecessary_distance_mm: Some(200.0),
                yielding,
            },
            Top,
        );
        for (development_length_mm, expected) in [(1600.0, None), (310.0, Some(false))] {
            let cutoff = input(
                RcUltimateBondBar::Cutoff {
                    development_length_mm,
                    clear_span_mm: 3000.0,
                    unnecessary_distance_mm: 200.0,
                    stress,
                },
                Top,
            );
            let checked = directional(
                &model,
                RcUltimateBeamBondDemand {
                    positive: Some(RcUltimateBeamBondDirectionDemand {
                        bars: RcUltimateBondDemand::ThroughAndCutoff { through, cutoff },
                        ..positive
                    }),
                    ..supplied
                },
            );
            let by_direction = checked.beam_bond.unwrap();
            let positive = by_direction.positive;
            assert_eq!(positive.development_length_ok, expected);
            assert_eq!(checked.bond_development_length_ok, expected);
            assert!(
                positive.stress.is_none()
                    && positive.through_sigma_d_mpa.is_none()
                    && positive.cutoff_sigma_d_mpa.is_none(),
                "{checked:?}"
            );
            assert!(by_direction.negative.stress.is_some() && checked.bond_stress.is_some());
            assert!(checked.detail.contains("引張側の主筋が2本未満"));
        }
    }
}

#[test]
fn same_sign_moments_confirm_the_length_scope_without_calculating_bar_stress() {
    use RcUltimateBeamBondTensionSide::Top;
    let model = model();
    let supplied = demand();
    let positive = supplied.positive.unwrap();
    let invalid_factor = RcUltimateBondElasticStress {
        lateral_factor: 1.49,
        ..top_stress()
    };
    for (yielding, stress) in [
        (
            RcUltimateThroughBarYield::Unverified,
            RcUltimateCutoffBarStress::Unverified,
        ),
        (
            RcUltimateThroughBarYield::WithoutFlexuralYield {
                moment_distribution: RcUltimateBondMomentDistribution::NearAntisymmetric,
                stress: invalid_factor,
            },
            RcUltimateCutoffBarStress::WithoutFlexuralYield(invalid_factor),
        ),
    ] {
        for (bar, absent, length) in [
            (
                RcUltimateBondBar::Through {
                    clear_span_mm: 3000.0,
                    cutoff_unnecessary_distance_mm: None,
                    yielding,
                },
                true,
                None,
            ),
            (
                RcUltimateBondBar::Cutoff {
                    development_length_mm: 1600.0,
                    clear_span_mm: 3000.0,
                    unnecessary_distance_mm: 200.0,
                    stress,
                },
                false,
                Some(true),
            ),
        ] {
            let checked = directional(
                &model,
                RcUltimateBeamBondDemand {
                    positive: Some(RcUltimateBeamBondDirectionDemand {
                        bars: RcUltimateBondDemand::Single(input(bar, Top)),
                        ..positive
                    }),
                    ..supplied
                },
            );
            let positive = checked.beam_bond.unwrap().positive;
            assert!(positive.stress.is_none() && positive.unchecked_reason.is_some());
            assert_eq!(checked.bond_cutoff_absent, absent, "{checked:?}");
            assert_eq!(checked.bond_development_length_ok, length, "{checked:?}");
        }
    }
}

#[test]
fn explicit_yield_conflicts_with_directional_bond_even_when_both_moments_are_zero() {
    use RcUltimateBeamBondTensionSide::Top;
    let model = model();
    let supplied = demand();
    let positive = supplied.positive.unwrap();
    for (development_length_mm, expected) in [(1600.0, None), (310.0, Some(false))] {
        let cutoff = |stress| {
            input(
                RcUltimateBondBar::Cutoff {
                    development_length_mm,
                    clear_span_mm: 3000.0,
                    unnecessary_distance_mm: 200.0,
                    stress,
                },
                Top,
            )
        };
        let pair = |yielding| RcUltimateBondDemand::ThroughAndCutoff {
            through: input(
                RcUltimateBondBar::Through {
                    clear_span_mm: 3000.0,
                    cutoff_unnecessary_distance_mm: Some(200.0),
                    yielding,
                },
                Top,
            ),
            cutoff: cutoff(RcUltimateCutoffBarStress::Unverified),
        };
        for bars in [
            RcUltimateBondDemand::Single(cutoff(RcUltimateCutoffBarStress::FlexuralYield)),
            pair(RcUltimateThroughBarYield::BothEnds),
            pair(RcUltimateThroughBarYield::OneEndWithOtherElastic),
        ] {
            let checked = directional(
                &model,
                RcUltimateBeamBondDemand {
                    positive: Some(RcUltimateBeamBondDirectionDemand {
                        section: RcUltimateBeamBondSection {
                            long_term_mz_nmm: 0.0,
                            lateral_mz_nmm: 0.0,
                            ..positive.section
                        },
                        bars,
                        ..positive
                    }),
                    ..supplied
                },
            );
            let result = checked.beam_bond.unwrap().positive;
            assert_eq!(result.development_length_ok, expected, "{checked:?}");
            assert_eq!(checked.bond_development_length_ok, expected, "{checked:?}");
            assert!(result.stress.is_none());
            assert!(result
                .unchecked_reason
                .unwrap()
                .contains("曲げ降伏条件が矛盾"));
        }
    }
}

#[test]
fn missing_member_face_keeps_the_requested_section_and_directional_reasons() {
    let mut model = model();
    model.elements[0].rigid_zone.face_j = None;
    for base in [
        MemberDemand {
            mz: 49.5e6,
            q_long: Some(0.0),
            ..Default::default()
        },
        total(),
    ] {
        let checked = check(
            &model,
            MemberDemand {
                beam_bond: Some(demand()),
                ..base
            },
        );
        let reason = checked.unchecked_reason.as_deref().unwrap();
        assert!(
            reason.contains("ultimate_clear_span_uncomputed: j端"),
            "{checked:?}"
        );
        assert!(reason.contains("付着照査の内法長さ"), "{checked:?}");
        let directions = checked.beam_bond.unwrap();
        for (result, distance, tension_side) in [
            (directions.positive, 0.0, RcUltimateBeamBondTensionSide::Top),
            (
                directions.negative,
                3000.0,
                RcUltimateBeamBondTensionSide::Bottom,
            ),
        ] {
            let section = result.section.unwrap();
            assert_eq!(
                (section.distance_from_i_face_mm, section.tension_side),
                (distance, tension_side)
            );
            assert!(result.through_sigma_d_mpa.is_none() && result.stress.is_none());
            assert!(result
                .unchecked_reason
                .unwrap()
                .contains("付着照査の内法長さ"));
        }
        assert!(checked.detail.contains("X軸の正地震荷重"));
        assert!(checked.detail.contains("X軸の負地震荷重"));
        assert!(!checked.ok);
    }
}

#[test]
fn conflicting_legacy_input_can_prove_only_independent_development_length_failure() {
    for (length, expected) in [(310.0, Some(false)), (700.0, None)] {
        let cutoff = input(
            RcUltimateBondBar::Cutoff {
                development_length_mm: length,
                clear_span_mm: 3000.0,
                unnecessary_distance_mm: 200.0,
                stress: RcUltimateCutoffBarStress::FlexuralYield,
            },
            RcUltimateBeamBondTensionSide::Top,
        );
        let checked = check(
            &model(),
            MemberDemand {
                beam_bond: Some(demand()),
                bond: Some(RcUltimateBondDemand::Single(cutoff)),
                ..total()
            },
        );
        assert_eq!(checked.bond_development_length_ok, expected);
        assert!(!checked.ok && checked.bond_stress.is_none() && checked.bond_margin.is_nan());
        assert_eq!(checked.unchecked_reason.is_none(), expected == Some(false));
        assert_eq!(
            checked.detail.contains("必要な延長を満たしません"),
            expected == Some(false)
        );
        let directions = checked.beam_bond.unwrap();
        for direction in [directions.positive, directions.negative] {
            assert!(direction.through_sigma_d_mpa.is_none() && direction.stress.is_none());
            assert!(direction
                .unchecked_reason
                .unwrap()
                .contains("方向を指定しない付着照査入力が併用"));
        }
    }
}

#[test]
fn cutoff_distances_cannot_reach_past_either_face_from_the_verification_section() {
    use RcUltimateBeamBondTensionSide::{Bottom, Top};
    let model = model();
    let both = demand();
    let positive = both.positive.unwrap();
    let cutoff = |ld, l_prime, side| {
        input(
            RcUltimateBondBar::Cutoff {
                development_length_mm: ld,
                clear_span_mm: 3000.0,
                unnecessary_distance_mm: l_prime,
                stress: RcUltimateCutoffBarStress::WithoutFlexuralYield(
                    RcUltimateBondElasticStress {
                        lateral_factor: 4.0,
                        ..top_stress()
                    },
                ),
            },
            side,
        )
    };
    let at_center = |bars, negative| {
        directional(
            &model,
            RcUltimateBeamBondDemand {
                positive: Some(RcUltimateBeamBondDirectionDemand {
                    section: RcUltimateBeamBondSection {
                        distance_from_i_face_mm: 1500.0,
                        ..positive.section
                    },
                    bars,
                    ..positive
                }),
                negative,
                ..both
            },
        )
    };
    let at_limit = at_center(
        RcUltimateBondDemand::Single(cutoff(1500.0, 200.0, Top)),
        both.negative,
    );
    let at_limit_result = at_limit.beam_bond.unwrap().positive;
    assert_eq!(at_limit_result.unchecked_reason, None);
    assert_eq!(at_limit_result.development_length_ok, Some(true));
    let stress = at_limit_result.stress.unwrap();
    assert!((stress.demand_mpa - 1.4923522116577097).abs() < 1e-12);
    assert!(stress.demand_mpa > stress.capacity_mpa && !at_limit.ok);

    let beyond = at_center(
        RcUltimateBondDemand::Single(cutoff(2000.0, 200.0, Top)),
        both.negative,
    );
    let result = beyond.beam_bond.unwrap().positive;
    assert!(result.stress.is_none() && result.development_length_ok.is_none());
    assert!(result
        .unchecked_reason
        .unwrap()
        .contains("付着長さが付着検定断面から到達"));
    assert!(!beyond.ok && beyond.unchecked_reason.is_some());

    let bad_distance = 1500.1;
    let misplaced_cutoff = cutoff(1500.0, bad_distance, Top);
    let through = input(
        RcUltimateBondBar::Through {
            clear_span_mm: 3000.0,
            cutoff_unnecessary_distance_mm: Some(bad_distance),
            yielding: RcUltimateThroughBarYield::WithoutFlexuralYield {
                moment_distribution: RcUltimateBondMomentDistribution::NearAntisymmetric,
                stress: RcUltimateBondElasticStress {
                    lateral_factor: 4.0,
                    ..top_stress()
                },
            },
        },
        Top,
    );
    let negative = both.negative.unwrap();
    let independently_short = Some(RcUltimateBeamBondDirectionDemand {
        bars: RcUltimateBondDemand::Single(input(
            RcUltimateBondBar::Cutoff {
                development_length_mm: 310.0,
                clear_span_mm: 3000.0,
                unnecessary_distance_mm: 200.0,
                stress: RcUltimateCutoffBarStress::Unverified,
            },
            Bottom,
        )),
        ..negative
    });
    for bars in [
        RcUltimateBondDemand::Single(misplaced_cutoff),
        RcUltimateBondDemand::ThroughAndCutoff {
            through,
            cutoff: misplaced_cutoff,
        },
    ] {
        let checked = at_center(bars, independently_short);
        let directions = checked.beam_bond.unwrap();
        assert!(directions.positive.stress.is_none());
        assert!(directions.positive.development_length_ok.is_none());
        assert!(directions
            .positive
            .unchecked_reason
            .unwrap()
            .contains("不要断面までの距離が付着検定断面から到達"));
        assert_eq!(directions.negative.development_length_ok, Some(false));
        assert_eq!(checked.bond_development_length_ok, Some(false));
        assert!(!checked.ok && checked.unchecked_reason.is_none());
    }
}

#[test]
fn reaching_yield_before_amplification_contradicts_non_yielding_without_hiding_length_ng() {
    use RcUltimateBeamBondTensionSide::{Bottom, Top};
    let model = model();
    let both = demand();
    let positive = both.positive.unwrap();
    let negative = both.negative.unwrap();
    let stress = |lateral_tensile_stress_mpa| RcUltimateBondElasticStress {
        long_term_tensile_stress_mpa: 80.0,
        lateral_tensile_stress_mpa,
        lateral_factor: 2.0,
    };
    let cutoff = |development_length_mm, stress, side| {
        RcUltimateBondDemand::Single(input(
            RcUltimateBondBar::Cutoff {
                development_length_mm,
                clear_span_mm: 3000.0,
                unnecessary_distance_mm: 200.0,
                stress,
            },
            side,
        ))
    };
    let check = |bars, negative| {
        directional(
            &model,
            RcUltimateBeamBondDemand {
                positive: Some(RcUltimateBeamBondDirectionDemand { bars, ..positive }),
                negative: Some(negative),
                ..both
            },
        )
    };

    let below_yield = check(through(stress(299.0), Top), negative);
    let result = below_yield.beam_bond.unwrap().positive;
    assert_eq!(result.through_sigma_d_mpa, Some(380.0));
    assert!(result.stress.is_some() && result.unchecked_reason.is_none());

    let RcUltimateBondDemand::Single(cutoff_with_low_yield) = cutoff(
        1600.0,
        RcUltimateCutoffBarStress::WithoutFlexuralYield(stress(120.0)),
        Top,
    ) else {
        unreachable!()
    };
    let unverified_design_strength = check(
        RcUltimateBondDemand::Single(RcUltimateBondInput {
            design_yield_stress_mpa: Some(100.0),
            ..cutoff_with_low_yield
        }),
        negative,
    );
    let result = unverified_design_strength.beam_bond.unwrap().positive;
    assert!(result.stress.is_none());
    assert_eq!(result.development_length_ok, Some(true));
    assert_eq!(
        unverified_design_strength.bond_development_length_ok,
        Some(true)
    );
    assert!(result
        .unchecked_reason
        .unwrap()
        .contains("規格降伏強度を下回っています"));

    for lateral in [300.0, 320.0] {
        let invalid = check(through(stress(lateral), Top), negative);
        let result = invalid.beam_bond.unwrap();
        assert!(result.positive.through_sigma_d_mpa.is_none() && result.positive.stress.is_none());
        assert!(result
            .positive
            .unchecked_reason
            .unwrap()
            .contains("割増前の鉄筋応力度が設計用降伏応力度に達し"));
        assert_eq!(result.negative.through_sigma_d_mpa, Some(380.0));
        assert!(!invalid.ok && invalid.unchecked_reason.is_some() && !invalid.bond_cutoff_absent);
        let sufficient = check(
            cutoff(
                1600.0,
                RcUltimateCutoffBarStress::WithoutFlexuralYield(stress(lateral)),
                Top,
            ),
            negative,
        );
        assert!(sufficient.bond_development_length_ok.is_none());
        assert!(sufficient
            .beam_bond
            .unwrap()
            .positive
            .development_length_ok
            .is_none());
    }

    let paired_through = input(
        RcUltimateBondBar::Through {
            clear_span_mm: 3000.0,
            cutoff_unnecessary_distance_mm: Some(200.0),
            yielding: RcUltimateThroughBarYield::WithoutFlexuralYield {
                moment_distribution: RcUltimateBondMomentDistribution::NearAntisymmetric,
                stress: stress(299.0),
            },
        },
        Top,
    );
    for (ld, effective_depth_mm, cutoff_stress, through_ng_kept, expected_length, reason) in [
        (
            3100.0,
            290.5,
            stress(320.0),
            true,
            None,
            "付着長さが付着検定断面から到達できる内法区間を外れています",
        ),
        (
            250.0,
            290.5,
            stress(320.0),
            true,
            Some(false),
            "付着長さから有効せいを引いた長さを正の値として確認できません",
        ),
        (
            290.5,
            290.4999999,
            stress(320.0),
            true,
            Some(false),
            "付着長さから有効せいを引いた長さを正の値として確認できません",
        ),
        (
            290.5,
            290.4999999,
            stress(299.0),
            true,
            Some(false),
            "付着長さから有効せいを引いた長さを正の値として確認できません",
        ),
        (
            1600.0,
            290.5,
            RcUltimateBondElasticStress {
                long_term_tensile_stress_mpa: 0.0,
                ..stress(380.0)
            },
            true,
            Some(true),
            "曲げと鉄筋応力度の一方のみがゼロ",
        ),
        (
            1600.0,
            290.5,
            stress(320.0),
            false,
            None,
            "割増前の鉄筋応力度が設計用降伏応力度に達し",
        ),
    ] {
        let RcUltimateBondDemand::Single(cutoff) = cutoff(
            ld,
            RcUltimateCutoffBarStress::WithoutFlexuralYield(cutoff_stress),
            Top,
        ) else {
            unreachable!()
        };
        let checked = check(
            RcUltimateBondDemand::ThroughAndCutoff {
                through: paired_through,
                cutoff: RcUltimateBondInput {
                    effective_depth_mm,
                    ..cutoff
                },
            },
            negative,
        );
        let result = checked.beam_bond.unwrap().positive;
        assert_eq!(result.stress.is_some(), through_ng_kept, "{checked:?}");
        assert!(result.cutoff_sigma_d_mpa.is_none(), "{checked:?}");
        if let Some(stress) = result.stress {
            assert!(
                (stress.demand_mpa - 1.4385335724247859).abs() < 1e-12,
                "{checked:?}"
            );
        }
        assert_eq!(checked.bond_margin < 1.0, through_ng_kept, "{checked:?}");
        assert_eq!(
            checked.unchecked_reason.is_none(),
            through_ng_kept,
            "{checked:?}"
        );
        assert_eq!(result.development_length_ok, expected_length, "{checked:?}");
        assert_eq!(
            checked.bond_development_length_ok, expected_length,
            "{checked:?}"
        );
        assert!(
            result.unchecked_reason.unwrap().contains(reason),
            "{checked:?}"
        );
    }

    for (bars, negative, length_ng_in_positive) in [
        (
            cutoff(
                310.0,
                RcUltimateCutoffBarStress::WithoutFlexuralYield(stress(320.0)),
                Top,
            ),
            negative,
            true,
        ),
        (
            through(stress(320.0), Top),
            RcUltimateBeamBondDirectionDemand {
                bars: cutoff(310.0, RcUltimateCutoffBarStress::Unverified, Bottom),
                ..negative
            },
            false,
        ),
    ] {
        let checked = check(bars, negative);
        let result = checked.beam_bond.unwrap();
        assert!(result.positive.stress.is_none());
        assert!(result
            .positive
            .unchecked_reason
            .unwrap()
            .contains("割増前の鉄筋応力度が設計用降伏応力度に達し"));
        let ng = if length_ng_in_positive {
            result.positive
        } else {
            result.negative
        };
        assert_eq!(ng.development_length_ok, Some(false));
        assert_eq!(checked.bond_development_length_ok, Some(false));
        assert!(!checked.ok && checked.unchecked_reason.is_none());
    }
}

#[test]
fn directional_bond_requires_a_uniform_standalone_section_and_one_beam_element() {
    use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId, SlabId};
    use sepika_core::model::{
        Haunch, LoadTransfer, MemberDetailAttr, RegionAnchor, Slab, SlabPlate, SlabShape,
    };
    use RcUltimateBeamBondTensionSide::{Bottom, Top};

    let plain = model();
    let mut beam = demand();
    let cutoff = |length_mm, stress, side| {
        RcUltimateBondDemand::Single(input(
            RcUltimateBondBar::Cutoff {
                development_length_mm: length_mm,
                clear_span_mm: 3000.0,
                unnecessary_distance_mm: 200.0,
                stress,
            },
            side,
        ))
    };
    let mut positive = beam.positive.unwrap();
    positive.bars = cutoff(
        1700.0,
        RcUltimateCutoffBarStress::WithoutFlexuralYield(RcUltimateBondElasticStress {
            lateral_factor: 4.0,
            ..top_stress()
        }),
        Top,
    );
    beam.positive = Some(positive);

    let accepted = directional(&plain, beam);
    let at_plain_i = accepted.beam_bond.unwrap().positive;
    let stress = at_plain_i.stress.unwrap();
    assert!((stress.demand_mpa - 1.2805959560127704).abs() < 1e-12);
    assert!(stress.demand_mpa < stress.capacity_mpa && at_plain_i.unchecked_reason.is_none());
    assert_eq!(at_plain_i.development_length_ok, Some(true));

    let mut negative = beam.negative.unwrap();
    negative.bars = cutoff(310.0, RcUltimateCutoffBarStress::Unverified, Bottom);
    beam.negative = Some(negative);
    assert_eq!(
        directional(&plain, beam).bond_development_length_ok,
        Some(false)
    );

    let haunched = |depth_increase, width_increase| {
        let mut model = plain.clone();
        model.member_detail_attrs.push(MemberDetailAttr {
            haunch_i: Some(Haunch {
                length: 600.0,
                depth_increase,
                width_increase,
            }),
            ..MemberDetailAttr::new(ElemId(0))
        });
        model
    };
    let mut grouped = plain.clone();
    let mut end = grouped.nodes[1].clone();
    end.id = NodeId(2);
    end.coord[0] = 6000.0;
    grouped.nodes.push(end);
    let mut segment = grouped.elements[0].clone();
    segment.id = ElemId(1);
    segment.nodes[0] = NodeId(1);
    segment.nodes[1] = NodeId(2);
    grouped.elements.push(segment);
    grouped.girder_groups = vec![vec![ElemId(0), ElemId(1)]];

    let mut slab = plain.clone();
    let mut section =
        SectionShape::RcSlab { thickness: 150.0 }.to_section(SectionId(1), "S15".into());
    section.material = Some(MaterialId(0));
    slab.sections.push(section);
    slab.slabs.push(Slab {
        id: SlabId(0),
        shape: SlabShape::Attached {
            anchor: RegionAnchor::Line {
                nodes: [NodeId(0), NodeId(1)],
                span: [0.0, 1.0],
                transfer: LoadTransfer::Anchor,
            },
            extent: [1500.0; 2],
        },
        plate: SlabPlate {
            section: Some(SectionId(1)),
            ..Default::default()
        },
        reference_story: None,
        vertical_eccentricity_mm: None,
        tip_loads: Vec::new(),
    });

    for (model, reason) in [
        (haunched(100.0, 0.0), "ハンチ付きRC梁"),
        (haunched(0.0, 100.0), "ハンチ付きRC梁"),
        (grouped, "複数要素で構成する一本部材グループ"),
        (slab, "スラブ付きRC梁"),
    ] {
        model.validate().unwrap();
        let checks = |demand| {
            let demands = model
                .elements
                .iter()
                .map(|elem| {
                    (
                        elem.id,
                        if elem.id == ElemId(0) {
                            demand
                        } else {
                            total()
                        },
                    )
                })
                .collect::<Vec<_>>();
            collect_rc_ultimate_checks(&model, &demands, &UltimateShearOptions::default())
                .unwrap()
                .remove(0)
        };
        let result = checks(MemberDemand {
            beam_bond: Some(beam),
            ..total()
        });
        let directions = result.beam_bond.unwrap();
        for (direction, expected_side) in
            [(directions.positive, Top), (directions.negative, Bottom)]
        {
            assert_eq!(direction.section.unwrap().tension_side, expected_side);
            assert!(
                direction.unchecked_reason.unwrap().contains(reason),
                "{result:?}"
            );
            assert!(direction.cutoff_sigma_d_mpa.is_none() && direction.stress.is_none());
            assert!(direction.development_length_ok.is_none());
        }
        assert!(result.unchecked_reason.unwrap().contains(reason));
        assert!(result.bond_stress.is_none() && result.bond_development_length_ok.is_none());
        assert!(!result.ok);

        let legacy = checks(MemberDemand {
            bond: Some(RcUltimateBondDemand::Single(input(
                RcUltimateBondBar::Cutoff {
                    development_length_mm: 1700.0,
                    clear_span_mm: 3000.0,
                    unnecessary_distance_mm: 200.0,
                    stress: RcUltimateCutoffBarStress::FlexuralYield,
                },
                Top,
            ))),
            ..total()
        });
        assert!(legacy.beam_bond.is_none() && legacy.bond_stress.is_some());
        assert_eq!(legacy.bond_development_length_ok, Some(true));
        assert!(legacy.qsu.is_nan() && result.qsu.is_nan());
    }
}

#[test]
fn directional_hinge_input_conflicts_only_with_bond_on_the_same_axis_and_direction() {
    for (development_length_mm, length_ok) in [(310.0, false), (1600.0, true)] {
        let mut bond = demand();
        let mut positive = bond.positive.unwrap();
        positive.bars = RcUltimateBondDemand::Single(input(
            RcUltimateBondBar::Cutoff {
                development_length_mm,
                clear_span_mm: 3000.0,
                unnecessary_distance_mm: 200.0,
                stress: RcUltimateCutoffBarStress::WithoutFlexuralYield(top_stress()),
            },
            RcUltimateBeamBondTensionSide::Top,
        ));
        bond.positive = Some(positive);
        let baseline = directional(&model(), bond);
        assert_eq!(baseline.bond_development_length_ok, Some(length_ok));
        for seismic_axis in [SeismicDir::X, SeismicDir::Y] {
            let checked = check(
                &model(),
                MemberDemand {
                    beam_bond: Some(bond),
                    beam_hinge_shear: Some(RcUltimateBeamHingeShearDemand {
                        seismic_axis,
                        face_positions_mm: [0.0, 3000.0],
                        q0_n: [0.0; 2],
                        end_envelope_confirmed: true,
                        positive: Some(RcUltimateBeamHingeShearDirectionDemand {
                            mechanism: StaticBeamHingeMechanism::ITopJBottom,
                            yield_moment_bounds_nmm: [100e6; 2],
                            support_transfer_confirmed: true,
                        }),
                        negative: None,
                    }),
                    ..total()
                },
            );
            let expected_length =
                (!length_ok || seismic_axis != SeismicDir::X).then_some(length_ok);
            assert_eq!(checked.bond_development_length_ok, expected_length);
            if !length_ok {
                assert!(!checked.ok && checked.unchecked_reason.is_none());
            }
            let by_direction = checked.beam_bond.unwrap();
            let hinge = checked.beam_hinge_shear.unwrap();
            let positive_reason = hinge.positive.unwrap_err();
            if seismic_axis == SeismicDir::X {
                assert!(positive_reason.contains("非降伏と両端曲げ降伏"));
                assert!(by_direction
                    .positive
                    .unchecked_reason
                    .unwrap()
                    .contains("非降伏と両端曲げ降伏"));
                assert!(by_direction.positive.stress.is_none());
            } else {
                assert!(positive_reason.contains("接続柱がありません"));
                assert!(by_direction.positive.unchecked_reason.is_none());
                assert!(by_direction.positive.stress.is_some());
            }
            assert_eq!(by_direction.positive.development_length_ok, expected_length);
            assert_eq!(by_direction.negative.through_sigma_d_mpa, Some(380.0));
            assert!(by_direction.negative.unchecked_reason.is_none());
            assert!(hinge
                .negative
                .unwrap_err()
                .contains("この方向の両端曲げ降伏"));
        }
    }
}
