use sepika_core::dof::DofMap;
use sepika_core::ids::MaterialId;
use sepika_core::model::{ElementKind, ForceRegime, Material, MaterialCategory};
use sepika_core::section_shape::SectionShape;
use sepika_solver::common::constraint::Reducer;
use sepika_solver::nonlinear::pushover::pushover_analysis;
use sepika_solver::statics::analysis::SeismicDir;

#[path = "support/pushover_column_model.rs"]
mod pushover_column_model;
use pushover_column_model::rc_hinge_model;

#[test]
fn direct_input_main_rebar_strength_is_used_by_fiber_ms_and_mn_surface() {
    use sepika_core::model::AnalysisKind;
    use sepika_element::behavior::{Ctx, LocalVec};
    use sepika_element::factory::{
        build_hinge_view, build_nonlinear_behavior, ensure_nonlinear_input_with_basis,
        AnalysisHingeModel, StrengthBasis,
    };

    let (rc, rebar, b, d) = rc_hinge_model();
    let mut src = rc.clone();
    src.sections[0].shape = Some(SectionShape::SrcColumnRect {
        b,
        d,
        rebar: rebar.clone(),
        steel_height: 400.0,
        steel_width: 200.0,
        steel_web_thick: 8.0,
        steel_flange_thick: 13.0,
    });
    src.sections[0].steel_material = Some(MaterialId(3));
    src.materials.push(Material {
        id: MaterialId(3),
        name: "SN400B".into(),
        category: MaterialCategory::Steel,
        fy: Some(235.0),
        strength_factor: Some(1.05),
        ..rc.materials[1].clone()
    });
    let src_steel_area_mm2 = 8192.0;

    for (mut model, steel_area_mm2) in [(rc, 0.0), (src, src_steel_area_mm2)] {
        for (element_kind, view_kind) in [
            (ElementKind::Fiber, AnalysisHingeModel::Fiber),
            (ElementKind::MultiSpring, AnalysisHingeModel::MultiSpring),
        ] {
            model.elements[0].kind = element_kind;
            for (name, concrete_factor, rebar_factor, basis, kind, expected_fy) in [
                (
                    "直接入力主筋",
                    Some(1.2),
                    Some(1.0),
                    StrengthBasis::MaterialStrength,
                    AnalysisKind::Incremental,
                    345.0,
                ),
                (
                    "直接入力主筋",
                    None,
                    Some(1.0),
                    StrengthBasis::MaterialStrength,
                    AnalysisKind::Incremental,
                    345.0,
                ),
                (
                    "直接入力主筋",
                    Some(1.0),
                    None,
                    StrengthBasis::MaterialStrength,
                    AnalysisKind::Incremental,
                    379.5,
                ),
                (
                    "直接入力主筋",
                    Some(1.2),
                    Some(1.05),
                    StrengthBasis::MaterialStrength,
                    AnalysisKind::Incremental,
                    362.25,
                ),
                (
                    "直接入力主筋",
                    Some(1.2),
                    Some(1.05),
                    StrengthBasis::Nominal,
                    AnalysisKind::TimeHistory,
                    345.0,
                ),
                (
                    "SD345",
                    Some(1.2),
                    Some(1.0),
                    StrengthBasis::MaterialStrength,
                    AnalysisKind::Incremental,
                    345.0,
                ),
            ] {
                model.materials[0].strength_factor = concrete_factor;
                model.materials[1].name = name.into();
                model.materials[1].strength_factor = rebar_factor;

                ensure_nonlinear_input_with_basis(&model, kind, basis).unwrap();
                let data = &model.elements[0];
                let mut element = build_nonlinear_behavior(data, &model, basis, kind, None);
                let mut axial_extension = LocalVec {
                    data: smallvec::smallvec![0.0; 12],
                };
                axial_extension.data[8] = 0.03;
                element
                    .update_state(&axial_extension, false, &Ctx { model: &model })
                    .unwrap();
                let states = element.fiber_section_states().unwrap();
                for section in states {
                    let mut main_bars = section
                        .fibers
                        .iter()
                        .filter(|fiber| fiber.material == 1)
                        .peekable();
                    assert!(main_bars.peek().is_some());
                    for bar in main_bars {
                        let fy = bar.strain.abs() * 205_000.0 / bar.yield_ratio;
                        assert!((fy - expected_fy).abs() < 1e-9, "{element_kind:?} {name} {concrete_factor:?} {rebar_factor:?} {basis:?}: {fy} != {expected_fy}");
                    }
                }

                let view = build_hinge_view(data, &model, basis, kind, 0.0, 8, 8).unwrap();
                assert_eq!(view.model, view_kind);
                let n_tens = view.mn_surface.unwrap().n_tens;
                let steel_fy = match basis {
                    StrengthBasis::MaterialStrength => 246.75,
                    StrengthBasis::Nominal => 235.0,
                };
                let expected_n_tens =
                    rebar.total_main_area() * expected_fy + steel_area_mm2 * steel_fy;
                assert!((n_tens - expected_n_tens).abs() < 1e-6, "{element_kind:?} {name} {concrete_factor:?} {rebar_factor:?} {basis:?}: {n_tens} != {expected_n_tens}");
            }
        }
    }
}

#[test]
fn direct_input_main_rebar_invalid_strength_stops_pushover_and_hinge_view() {
    use sepika_core::model::AnalysisKind;
    use sepika_element::factory::{build_hinge_view, StrengthBasis};

    let (mut model, _, _, _) = rc_hinge_model();
    model.materials[1].name = "直接入力主筋".into();
    let dofmap = DofMap::build(&model);
    let reducer = Reducer::build(&model, &dofmap);

    for (kind, force_regime) in [
        (ElementKind::Fiber, ForceRegime::Auto),
        (ElementKind::MultiSpring, ForceRegime::Auto),
        (ElementKind::Beam, ForceRegime::AxialBendingInteract),
    ] {
        model.elements[0].kind = kind;
        model.elements[0].force_regime = force_regime;
        for (fy, factor, reason) in [
            (345.0, 0.0, "割増には正の有限値"),
            (345.0, -1.0, "割増には正の有限値"),
            (345.0, f64::NAN, "割増には正の有限値"),
            (345.0, f64::INFINITY, "割増には正の有限値"),
            (345.0, f64::MAX, "割増後の材料強度は正の有限値"),
            (0.25, f64::from_bits(1), "割増後の材料強度は正の有限値"),
            (f64::NAN, 1.0, "Fy には正の有限値"),
        ] {
            model.materials[1].strength_factor = Some(factor);
            model.materials[1].fy = Some(fy);
            let diagnostic = pushover_analysis(&model, &dofmap, &reducer, SeismicDir::X, 40, 60.0)
                .expect_err("直接入力した主筋の不正値を診断する");
            assert!(
                diagnostic.contains("部材 ID 0 の主筋の材料 ID 1「直接入力主筋」"),
                "{diagnostic}"
            );
            assert!(diagnostic.contains(reason), "{diagnostic}");
            let view = build_hinge_view(
                &model.elements[0],
                &model,
                StrengthBasis::MaterialStrength,
                AnalysisKind::Incremental,
                0.0,
                8,
                8,
            )
            .unwrap();
            assert!(view.mn_surface.is_none());
            assert_eq!(
                view.unavailability_reason.as_deref(),
                Some(diagnostic.as_str())
            );
        }
    }
}
