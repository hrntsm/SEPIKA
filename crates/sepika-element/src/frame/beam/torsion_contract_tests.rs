use super::{composite_props_of, BeamElement};
use crate::behavior::{Ctx, ElementBehavior, LocalVec};
use crate::factory::StrengthBasis;
use crate::frame::fiber::FiberBeam;
use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId};
use sepika_core::model::{
    AnalysisKind, ElementData, ElementKind, EndCondition, ForceRegime, LocalAxis, Material,
    MaterialCategory, Model, Node, PropertyBasis,
};
use sepika_core::section_shape::SectionShape;

fn model(shape: SectionShape, cft: bool) -> Model {
    let mut section = shape.to_section(SectionId(0), "ねじり断面".into());
    section.material = Some(MaterialId(if cft { 1 } else { 0 }));
    section.steel_material = cft.then_some(MaterialId(0));
    Model {
        nodes: (0..2)
            .map(|i| Node {
                id: NodeId(i),
                coord: [3000.0 * i as f64, 0.0, 0.0],
                restraint: if i == 0 {
                    Dof6Mask::FIXED
                } else {
                    Dof6Mask::FREE
                },
                mass: None,
                story: None,
                support_spring: None,
            })
            .collect(),
        sections: vec![section],
        materials: vec![
            Material {
                id: MaterialId(0),
                name: "鋼材".into(),
                category: MaterialCategory::Steel,
                young: 205000.0,
                poisson: 0.3,
                density: 7.85e-9,
                shear: None,
                fc: None,
                fy: Some(295.0),
                strength_factor: None,
                concrete_class: Default::default(),
            },
            Material {
                id: MaterialId(1),
                name: "Fc24".into(),
                category: MaterialCategory::Concrete,
                young: 23000.0,
                poisson: 0.2,
                density: 2.4e-9,
                shear: None,
                fc: Some(24.0),
                fy: None,
                strength_factor: None,
                concrete_class: Default::default(),
            },
        ],
        elements: vec![ElementData {
            id: ElemId(0),
            kind: ElementKind::Beam,
            nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 0.0, 1.0],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            plastic_zone: None,
            spring: None,
        }],
        ..Default::default()
    }
}

fn fiber(model: &Model) -> Result<FiberBeam, String> {
    FiberBeam::try_new(
        &model.elements[0],
        model,
        StrengthBasis::Nominal,
        AnalysisKind::Incremental,
    )
}

fn relative(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual / expected - 1.0).abs() <= tolerance,
        "actual={actual}, expected={expected}, tolerance={tolerance}"
    );
}

fn observe_torsion(element: &mut dyn ElementBehavior, model: &Model, gj_n_mm2: f64) -> f64 {
    let ctx = Ctx { model };
    let stiffness = element.tangent_stiffness(&ctx);
    let k = gj_n_mm2 / 3000.0;
    for (i, j, expected) in [(3, 3, k), (9, 9, k), (3, 9, -k), (9, 3, -k)] {
        relative(stiffness.get(i, j), expected, 1.0e-10);
    }
    let torque_n_mm = 1.0e6;
    let theta = torque_n_mm / stiffness.get(9, 9);
    relative(theta, torque_n_mm * 3000.0 / gj_n_mm2, 1.0e-8);
    let mut displacement = LocalVec {
        data: smallvec::SmallVec::from_elem(0.0, 12),
    };
    displacement.data[9] = theta;
    element.update_state(&displacement, false, &ctx);
    let force = element.internal_force(&ctx);
    relative(force.data[9], torque_n_mm, 1.0e-8);
    relative(force.data[3], -torque_n_mm, 1.0e-8);
    stiffness.get(9, 9) * 3000.0
}

#[test]
fn h_root_radius_preserves_conventional_j_in_actual_beam_and_fiber_torque_response() {
    for root_r in [0.0, 13.0] {
        let mut m = model(
            SectionShape::SteelH {
                height: 400.0,
                width: 200.0,
                web_thick: 8.0,
                flange_thick: 13.0,
                root_r: Some(root_r),
            },
            false,
        );
        m.sections[0].j = 123.0;
        relative(
            m.sections[0].resolved_j().unwrap(),
            356762.6666666667,
            1.0e-10,
        );
        let gj = m.materials[0].shear_modulus() * 356762.6666666667;
        observe_torsion(
            &mut BeamElement::try_new(&m.elements[0], &m).unwrap(),
            &m,
            gj,
        );
        observe_torsion(&mut fiber(&m).unwrap(), &m, gj);
        assert_eq!(m.sections[0].j, 123.0);
    }
}

#[test]
fn rounded_box_uses_current_shape_j_and_prioritizes_supplied_j_without_changing_cache() {
    let mut m = model(
        SectionShape::SteelBox {
            height: 400.0,
            width: 300.0,
            thick: 12.0,
            corner_r: Some(30.0),
        },
        false,
    );
    let old_j = 4.0 * 12.0 * (388.0_f64 * 288.0).powi(2) / (2.0 * (388.0 + 288.0));
    m.sections[0].j = old_j;
    let gs = m.materials[0].shear_modulus();
    for (basis, expected_j) in [
        (PropertyBasis::Shape, 453213215.4428274),
        (PropertyBasis::Supplied, old_j),
    ] {
        m.sections[0].property_basis.j = basis;
        observe_torsion(
            &mut BeamElement::try_new(&m.elements[0], &m).unwrap(),
            &m,
            gs * expected_j,
        );
        observe_torsion(&mut fiber(&m).unwrap(), &m, gs * expected_j);
        assert_eq!(m.sections[0].j, old_j);
    }
}

#[test]
fn cft_box_and_pipe_deliver_supplied_steel_j_plus_core_j_to_both_actual_elements() {
    for (shape, independent_core_j) in [
        (
            SectionShape::CftBox {
                height: 400.0,
                width: 300.0,
                thick: 12.0,
                corner_r: Some(0.0),
            },
            1449158972.0810955,
        ),
        (
            SectionShape::CftBox {
                height: 400.0,
                width: 300.0,
                thick: 12.0,
                corner_r: Some(30.0),
            },
            1448741968.108572,
        ),
        (
            SectionShape::CftPipe {
                outer_dia: 400.0,
                thick: 12.0,
            },
            1962236157.627097,
        ),
    ] {
        let core_j = shape.try_cft_core_torsion().unwrap().unwrap().j_mm4();
        relative(core_j, independent_core_j, 0.001);
        let mut m = model(shape, true);
        m.sections[0].property_basis.j = PropertyBasis::Supplied;
        let gamma_c = sepika_core::units::concrete_unit_weight_kn_m3(
            24.0,
            m.materials[1].concrete_class,
            sepika_core::units::ConcreteComposition::Plain,
        );
        let gc =
            sepika_core::section_shape::concrete_young_modulus_gamma(24.0, gamma_c) / (2.0 * 1.2);
        for steel_g in [None, Some(61_000.0)] {
            m.materials[0].shear = steel_g;
            let gs = m.materials[0].shear_modulus();
            let mut observed = Vec::new();
            for js in [100_000_000.0, 123_000_000.0] {
                m.sections[0].j = js;
                let gj = gs * js + gc * core_j;
                let mut beam = BeamElement::try_new(&m.elements[0], &m).unwrap();
                let mut fb = fiber(&m).unwrap();
                observed.push([
                    observe_torsion(&mut beam, &m, gj),
                    observe_torsion(&mut fb, &m, gj),
                ]);
                assert_eq!(m.sections[0].j, js);
            }
            for i in 0..2 {
                relative(observed[1][i] - observed[0][i], gs * 23_000_000.0, 1.0e-10);
            }
        }
    }
}

fn assert_both_rejected(m: &Model, reason: &str) {
    let beam_error = BeamElement::try_new(&m.elements[0], m)
        .err()
        .expect("Beam should reject");
    let fiber_error = fiber(m).err().expect("Fiber should reject");
    for error in [beam_error, fiber_error] {
        assert!(error.contains(reason), "{error}");
        assert!(
            error.contains("部材 ID 0") && error.contains("ねじり断面"),
            "{error}"
        );
    }
}

#[test]
fn pending_nonzero_cache_unknown_radius_and_invalid_cft_materials_are_errors() {
    let mut m = model(
        SectionShape::SteelBox {
            height: 400.0,
            width: 300.0,
            thick: 12.0,
            corner_r: Some(30.0),
        },
        false,
    );
    m.sections[0].j = 100_000_000.0;
    m.sections[0].property_basis.j = PropertyBasis::PendingShape;
    assert_both_rejected(&m, "未算定");
    m.sections[0].property_basis.j = PropertyBasis::Shape;
    if let Some(SectionShape::SteelBox { corner_r, .. }) = &mut m.sections[0].shape {
        *corner_r = None;
    }
    assert_both_rejected(&m, "角R");
    let base = model(
        SectionShape::CftPipe {
            outer_dia: 400.0,
            thick: 12.0,
        },
        true,
    );
    for missing in [true, false] {
        let mut m = base.clone();
        if missing {
            m.sections[0].steel_material = None;
        } else {
            m.materials[0].young = f64::NAN;
        }
        assert_both_rejected(&m, "鋼管");
        assert!(composite_props_of(&m, &m.elements[0]).is_err());
    }
    for fc in [None, Some(0.0), Some(f64::NAN)] {
        let mut m = base.clone();
        m.materials[1].fc = fc;
        assert_both_rejected(&m, "Fc");
        assert!(composite_props_of(&m, &m.elements[0]).is_err());
    }
    let mut m = base.clone();
    m.materials[0].poisson = -1.0;
    assert_both_rejected(&m, "ν");
    for gs in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let mut m = base.clone();
        m.materials[0].shear = Some(gs);
        assert_both_rejected(&m, "Gs");
        assert!(composite_props_of(&m, &m.elements[0]).is_err());
    }
    let mut m = base;
    m.sections[0].material = None;
    assert_both_rejected(&m, "Fc");
    assert!(composite_props_of(&m, &m.elements[0]).is_err());
}

#[test]
fn cft_core_numerical_failure_reaches_actual_beam_and_fiber_without_steel_only_fallback() {
    let shape = SectionShape::CftBox {
        height: 400.0,
        width: 1.0e-9,
        thick: 1.0e-10,
        corner_r: Some(0.0),
    };
    let core_error = shape.try_cft_core_torsion().unwrap_err();
    assert!(core_error.failed_attempt.is_some());
    let mut m = model(
        SectionShape::CftBox {
            height: 400.0,
            width: 300.0,
            thick: 12.0,
            corner_r: Some(0.0),
        },
        true,
    );
    m.sections[0].shape = Some(shape);
    m.sections[0].property_basis = Default::default();
    m.sections[0].j = 1.0e8;
    assert_both_rejected(&m, &core_error.reason);
    let error = composite_props_of(&m, &m.elements[0]).unwrap_err();
    assert!(error.contains(&core_error.reason), "{error}");
}
