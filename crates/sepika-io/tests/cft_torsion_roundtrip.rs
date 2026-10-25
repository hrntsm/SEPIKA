use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId};
use sepika_core::model::{
    AnalysisKind, ElementData, ElementKind, EndCondition, ForceRegime, LocalAxis, Material,
    MaterialCategory, Model, Node, PropertyBasis,
};
use sepika_core::section_shape::SectionShape;
use sepika_element::behavior::{Ctx, ElementBehavior, LocalVec};
use sepika_element::factory::StrengthBasis;
use sepika_element::frame::{beam::BeamElement, fiber::FiberBeam};
use sepika_io::ovika::{load_ovika, save_ovika, OvikaExtras, CURRENT_SCHEMA_VERSION};
use std::sync::atomic::{AtomicUsize, Ordering};

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
            vertical_eccentricity: Some(0.0),
            plastic_zone: None,
            spring: None,
        }],
        ..Default::default()
    }
}

fn relative(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual / expected - 1.0).abs() <= tolerance,
        "actual={actual}, expected={expected}, tolerance={tolerance}"
    );
}

fn roundtrip(model: &Model) -> Model {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "sepika416-torsion-{}-{}.ovika",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    assert_eq!(CURRENT_SCHEMA_VERSION, 1);
    let before = serde_json::to_value(model).unwrap();
    save_ovika(&path, model, OvikaExtras::default()).unwrap();
    assert_eq!(serde_json::to_value(model).unwrap(), before);
    let bytes = std::fs::read(&path).unwrap();
    let loaded = load_ovika(&path).unwrap();
    assert_eq!(serde_json::to_value(&loaded.model).unwrap(), before);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    let mut archive = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    let mut names: Vec<_> = archive
        .file_names()
        .map(|name| name.unwrap().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["manifest.json", "model.msgpack", "settings.json"]);
    let manifest: serde_json::Value =
        serde_json::from_reader(archive.by_name("manifest.json").unwrap()).unwrap();
    assert_eq!(manifest["schema_version"], 1);
    drop(archive);
    std::fs::remove_file(path).unwrap();
    loaded.model
}

fn actual_gj(model: &Model) -> [f64; 2] {
    let beam = BeamElement::try_new(&model.elements[0], model).unwrap();
    let fiber = FiberBeam::try_new(
        &model.elements[0],
        model,
        StrengthBasis::Nominal,
        AnalysisKind::Incremental,
    )
    .unwrap();
    let mut elements: [Box<dyn ElementBehavior>; 2] = [Box::new(beam), Box::new(fiber)];
    std::array::from_fn(|index| {
        let element = &mut elements[index];
        let ctx = Ctx { model };
        let stiffness = element.tangent_stiffness(&ctx);
        let gj = stiffness.get(9, 9) * 3000.0;
        assert!(gj.is_finite() && gj > 0.0);
        let torque_n_mm = 1.0e6;
        let theta = torque_n_mm * 3000.0 / gj;
        let mut displacement = LocalVec {
            data: smallvec::SmallVec::from_elem(0.0, 12),
        };
        displacement.data[9] = theta;
        element.update_state(&displacement, false, &ctx).unwrap();
        let force = element.internal_force(&ctx);
        relative(force.data[9], torque_n_mm, 1.0e-8);
        relative(force.data[3], -torque_n_mm, 1.0e-8);
        gj
    })
}

#[test]
fn current_ovika_preserves_h_box_radius_basis_supplied_j_and_shape_resolution() {
    for (shape, shape_j_mm4) in [
        (
            SectionShape::SteelH {
                height: 400.0,
                width: 200.0,
                web_thick: 8.0,
                flange_thick: 13.0,
                root_r: Some(13.0),
            },
            356762.6666666667,
        ),
        (
            SectionShape::SteelBox {
                height: 400.0,
                width: 300.0,
                thick: 12.0,
                horizontal_thick: 12.0,
                corner_r: Some(30.0),
            },
            453213215.4428274,
        ),
    ] {
        for basis in [PropertyBasis::Shape, PropertyBasis::Supplied] {
            let mut original = model(shape.clone(), false);
            original.sections[0].j = 123_000_000.0;
            original.sections[0].property_basis.j = basis;
            let loaded = roundtrip(&original);
            assert_eq!(loaded.sections[0], original.sections[0]);
            let expected_j = if basis == PropertyBasis::Shape {
                shape_j_mm4
            } else {
                123_000_000.0
            };
            relative(
                loaded.sections[0].resolved_j().unwrap(),
                expected_j,
                1.0e-10,
            );
            for gj in actual_gj(&loaded) {
                relative(
                    gj,
                    loaded.materials[0].shear_modulus() * expected_j,
                    1.0e-10,
                );
            }
            assert_eq!(loaded.sections[0].j, 123_000_000.0);
            assert_eq!(roundtrip(&loaded).sections[0], original.sections[0]);
        }
    }
}

#[test]
fn current_ovika_preserves_cft_steel_j_and_actual_steel_g_in_beam_and_fiber() {
    for (shape, independent_core_j_mm4) in [
        (
            SectionShape::CftPipe {
                outer_dia: 400.0,
                thick: 12.0,
            },
            1962236157.627097,
        ),
        (
            SectionShape::CftBox {
                height: 400.0,
                width: 300.0,
                thick: 12.0,
                horizontal_thick: 12.0,
                corner_r: Some(30.0),
            },
            1448741968.108572,
        ),
    ] {
        let mut original = model(shape, true);
        original.sections[0].property_basis.j = PropertyBasis::Supplied;
        let gamma_c = sepika_core::units::concrete_unit_weight_kn_m3(
            24.0,
            original.materials[1].concrete_class,
            sepika_core::units::ConcreteComposition::Plain,
        );
        let gc_mpa =
            sepika_core::section_shape::concrete_young_modulus_gamma(24.0, gamma_c) / (2.0 * 1.2);
        let mut values = Vec::new();
        for steel_g in [None, Some(61_000.0)] {
            original.materials[0].shear = steel_g;
            let gs_mpa = original.materials[0].shear_modulus();
            let mut observed = Vec::new();
            for steel_j_mm4 in [100_000_000.0, 123_000_000.0] {
                original.sections[0].j = steel_j_mm4;
                let loaded = roundtrip(&original);
                assert_eq!(loaded.sections[0], original.sections[0]);
                assert_eq!(loaded.materials, original.materials);
                let gj = actual_gj(&loaded);
                for value in gj {
                    relative(
                        value,
                        gs_mpa * steel_j_mm4 + gc_mpa * independent_core_j_mm4,
                        0.001,
                    );
                }
                assert_eq!(loaded.sections[0].j, steel_j_mm4);
                assert_eq!(roundtrip(&loaded).sections[0], original.sections[0]);
                observed.push(gj);
            }
            for (supplied, baseline) in observed[1].iter().zip(&observed[0]) {
                relative(supplied - baseline, gs_mpa * 23_000_000.0, 1.0e-10);
            }
            values.push(observed);
        }
        let default_gs = 205000.0 / (2.0 * 1.3);
        for (js_index, steel_j_mm4) in [100_000_000.0, 123_000_000.0].into_iter().enumerate() {
            for (explicit_g, default_g) in values[1][js_index].iter().zip(&values[0][js_index]) {
                relative(
                    explicit_g - default_g,
                    (61_000.0 - default_gs) * steel_j_mm4,
                    1.0e-10,
                );
            }
        }
    }
}
