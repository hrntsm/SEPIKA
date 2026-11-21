use sepika_core::ids::SectionId;
use sepika_core::model::{FrameSectionUse, Model, PropertyBasis, SectionPropertyBasis};
use sepika_core::section_shape::{
    BeamStirrup, CircleColumnHoop, RcBeamRebar, RcCircleColumnRebar, RcRectColumnRebar, RebarSize,
    RectColumnHoop, SectionShape,
};
use sepika_edit::{EditSectionShape, SectionField, SetSectionField, UndoStack};

const SECTION: SectionId = SectionId(0);
const DIRECT_J_MM4: f64 = 2.5e9;

fn edit_section(model: &mut Model, shape: SectionShape, frame_use: Option<FrameSectionUse>) {
    let mut history = UndoStack::new();
    assert!(
        history.run(
            model,
            Box::new(EditSectionShape {
                section: SECTION,
                new_shape: shape,
                frame_use,
            })
        ),
        "{:?}",
        history.last_error()
    );
}

fn model_with_direct_section_properties(shape: &SectionShape, frame_use: FrameSectionUse) -> Model {
    let mut section = shape.to_section(SECTION, "断面1".into());
    section.frame_use = Some(frame_use);
    section.floor = Some("2".into());
    let mut model = Model::default();
    model.sections.push(section);
    let mut history = UndoStack::new();
    for (field, value) in [
        (SectionField::Iy, 3.5e10),
        (SectionField::J, DIRECT_J_MM4),
        (SectionField::AsY, 2.1e5),
        (SectionField::PanelThickness, 24.0),
    ] {
        assert!(history.run(
            &mut model,
            Box::new(SetSectionField {
                id: SECTION,
                field,
                value,
            })
        ));
    }
    model
}

fn beam_rebar() -> RcBeamRebar {
    RcBeamRebar {
        main_dia: RebarSize::DiameterMm(19.0),
        top: vec![3],
        bottom: vec![4],
        cover: 40.0,
        stirrup: BeamStirrup {
            dia: RebarSize::DiameterMm(10.0),
            pitch: 150.0,
            legs: 2,
        },
    }
}

fn rect_column_rebar() -> RcRectColumnRebar {
    RcRectColumnRebar {
        main_dia: RebarSize::DiameterMm(19.0),
        x: vec![4],
        y: vec![4],
        cover: 40.0,
        hoop: RectColumnHoop {
            dia: RebarSize::DiameterMm(10.0),
            pitch: 150.0,
            legs_x: 2,
            legs_y: 2,
        },
    }
}

fn src_beam() -> SectionShape {
    SectionShape::SrcBeamRect {
        b: 600.0,
        d: 700.0,
        rebar: beam_rebar(),
        steel_height: 300.0,
        steel_width: 150.0,
        steel_web_thick: 10.0,
        steel_flange_thick: 16.0,
    }
}

#[test]
fn rebar_edits_preserve_properties_and_update_design_and_fiber_inputs() {
    use FrameSectionUse::{Column, Girder};
    use SectionShape::*;
    for (original, frame_use, old_count, new_count) in [
        (
            RcBeamRect {
                b: 600.0,
                d: 700.0,
                rebar: beam_rebar(),
            },
            Girder,
            7,
            9,
        ),
        (
            RcColumnRect {
                b: 600.0,
                d: 700.0,
                rebar: rect_column_rebar(),
            },
            Column,
            12,
            14,
        ),
        (
            RcColumnCircle {
                d: 600.0,
                rebar: RcCircleColumnRebar {
                    main_dia: RebarSize::DiameterMm(19.0),
                    count: 12,
                    cover: 40.0,
                    hoop: CircleColumnHoop {
                        dia: RebarSize::DiameterMm(10.0),
                        pitch: 150.0,
                    },
                },
            },
            Column,
            12,
            16,
        ),
        (src_beam(), Girder, 7, 9),
        (
            SrcColumnRect {
                b: 600.0,
                d: 700.0,
                rebar: rect_column_rebar(),
                steel_height: 300.0,
                steel_width: 150.0,
                steel_web_thick: 10.0,
                steel_flange_thick: 16.0,
            },
            Column,
            12,
            14,
        ),
    ] {
        original.validate_rebar().unwrap();
        let mut model = model_with_direct_section_properties(&original, frame_use);
        let before = model.sections[0].clone();
        edit_section(&mut model, original.clone(), Some(frame_use));
        assert_eq!(model.sections[0], before);

        let mut edited = original;
        match &mut edited {
            RcBeamRect { rebar, .. } | SrcBeamRect { rebar, .. } => {
                rebar.top = vec![5];
                rebar.stirrup.pitch = 100.0;
            }
            RcColumnRect { rebar, .. } | SrcColumnRect { rebar, .. } => {
                rebar.x = vec![5];
                rebar.hoop.pitch = 100.0;
            }
            RcColumnCircle { rebar, .. } => {
                rebar.count = 16;
                rebar.hoop.pitch = 100.0;
            }
            _ => unreachable!(),
        }
        edit_section(&mut model, edited.clone(), Some(frame_use));
        let actual = &model.sections[0];
        let shape = actual.shape.as_ref().unwrap();
        let mut expected = before.clone();
        expected.shape = Some(edited);
        assert_eq!(actual, &expected);
        assert_eq!(actual.resolved_j().unwrap(), DIRECT_J_MM4);

        for (shape, count) in [
            (before.shape.as_ref().unwrap(), old_count),
            (shape, new_count),
        ] {
            shape.validate_rebar().unwrap();
            let (main_area_mm2, positions) = match shape {
                RcBeamRect { b, d, rebar } | SrcBeamRect { b, d, rebar, .. } => {
                    (rebar.total_main_area(), rebar.bar_positions(*b, *d))
                }
                RcColumnRect { b, d, rebar } | SrcColumnRect { b, d, rebar, .. } => {
                    (rebar.total_main_area(), rebar.bar_positions(*b, *d))
                }
                RcColumnCircle { d, rebar } => (rebar.total_main_area(), rebar.bar_positions(*d)),
                _ => unreachable!(),
            };
            let expected_area_mm2 = count as f64 * std::f64::consts::PI * 9.5_f64.powi(2);
            assert!((main_area_mm2 - expected_area_mm2).abs() < 1e-9);
            assert_eq!(positions.unwrap().len(), count);
        }
    }
}

#[test]
fn changing_section_kind_dimensions_or_use_redefines_properties() {
    use FrameSectionUse::Girder;
    let original = src_beam();
    for changed_dimension in 0..6 {
        let mut edited = original.clone();
        let SectionShape::SrcBeamRect {
            b,
            d,
            steel_height,
            steel_width,
            steel_web_thick,
            steel_flange_thick,
            ..
        } = &mut edited
        else {
            unreachable!();
        };
        *[
            b,
            d,
            steel_height,
            steel_width,
            steel_web_thick,
            steel_flange_thick,
        ][changed_dimension] += 1.0;
        edited.validate_rebar().unwrap();

        let mut model = model_with_direct_section_properties(&original, Girder);
        edit_section(&mut model, edited.clone(), Some(Girder));
        let section = &model.sections[0];
        assert_eq!(section.property_basis, SectionPropertyBasis::SHAPE);
        assert_eq!(section.panel_thickness, None);
        assert_eq!(section.iy, edited.calc_iy());
        assert_eq!(section.resolved_j().unwrap(), edited.calc_j());
    }

    let rc_beam = SectionShape::RcBeamRect {
        b: 600.0,
        d: 700.0,
        rebar: beam_rebar(),
    };
    for (edited, frame_use) in [(rc_beam, Some(Girder)), (original.clone(), None)] {
        let mut model = model_with_direct_section_properties(&original, Girder);
        edit_section(&mut model, edited.clone(), frame_use);
        let section = &model.sections[0];
        assert_eq!(section.shape, Some(edited));
        assert_eq!(section.frame_use, frame_use);
        assert_eq!(section.property_basis.j, PropertyBasis::Shape);
        assert_eq!(section.panel_thickness, None);
    }
}
