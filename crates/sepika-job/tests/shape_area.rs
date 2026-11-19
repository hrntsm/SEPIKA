use sepika_core::{
    dof::{Dof, Dof6Mask},
    ids::{ElemId, LoadCaseId, MaterialId, NodeId, SectionId},
    model::*,
    section_shape::SectionShape,
};
use sepika_job::{compute::compute_linear_static, prepare::prepare_model_for_analysis};

const LENGTH_MM: f64 = 3000.0;
const YOUNG_N_PER_MM2: f64 = 205_000.0;
const LOAD_N: f64 = 1000.0;
const SHARP_AREA_MM2: f64 = 24_576.0;

fn brace_model() -> Model {
    let mut section = SectionShape::SteelBox {
        height: 400.0,
        width: 400.0,
        thick: 16.0,
        horizontal_thick: 16.0,
        corner_r: Some(40.0),
    }
    .to_section(SectionId(0), "BOX400".into());
    section.frame_use = Some(FrameSectionUse::Brace);
    section.material = Some(MaterialId(0));
    let mut axial_end = Dof6Mask::FIXED;
    axial_end.set_free(Dof::Ux);
    Model {
        nodes: [Dof6Mask::FIXED, axial_end]
            .into_iter()
            .enumerate()
            .map(|(index, restraint)| Node {
                id: NodeId(index as u32),
                coord: [index as f64 * LENGTH_MM, 0.0, 0.0],
                restraint,
                mass: None,
                story: None,
                support_spring: None,
            })
            .collect(),
        elements: vec![ElementData {
            id: ElemId(0),
            kind: ElementKind::Brace {
                tension_only: false,
            },
            nodes: vec![NodeId(0), NodeId(1)].into(),
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 0.0, 1.0],
            },
            end_cond: [EndCondition::Pinned; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            end_offsets_mm: [[0.0; 3]; 2],
            plastic_zone: None,
            spring: None,
        }],
        sections: vec![section],
        materials: vec![Material {
            id: MaterialId(0),
            name: "SN400B".into(),
            category: MaterialCategory::Steel,
            young: YOUNG_N_PER_MM2,
            poisson: 0.3,
            density: 7.85e-9,
            shear: None,
            fc: None,
            fy: Some(235.0),
            concrete_class: Default::default(),
            strength_factor: None,
        }],
        load_cases: vec![LoadCase {
            id: LoadCaseId(0),
            name: "引張軸力".into(),
            kind: LoadCaseKind::Other,
            origin: Default::default(),
            nodal: vec![NodalLoad::manual(
                NodeId(1),
                [LOAD_N, 0.0, 0.0, 0.0, 0.0, 0.0],
            )],
            member: vec![],
        }],
        ..Default::default()
    }
}

fn assert_linear_stiffness_and_weight(model: &Model, area_mm2: f64) {
    let result = compute_linear_static(model.clone(), LoadCaseId(0)).unwrap();
    let actual_stiffness = LOAD_N / result.disp[1][0];
    let expected_stiffness = YOUNG_N_PER_MM2 * area_mm2 / LENGTH_MM;
    assert!((actual_stiffness - expected_stiffness).abs() < 1e-6);

    let cfg = LoadCfg {
        steel_weight_factor: 1.0,
        ..Default::default()
    };
    let (nodal, member) = sepika_load::self_weight::self_weight_case_content(model, &cfg).unwrap();
    assert!(nodal.is_empty());
    let [weight] = member.as_slice() else {
        panic!("ブレース自重の部材荷重は1件です");
    };
    let MemberLoadKind::Distributed { a, b, w1, w2 } = weight.kind else {
        panic!("ブレース自重は等分布荷重です");
    };
    assert_eq!([a, b], [0.0, LENGTH_MM]);
    let expected_n_per_mm = 78.5e-6 * area_mm2;
    for w in [w1, w2] {
        assert!((w - expected_n_per_mm).abs() < 1e-10);
    }
}

#[test]
fn round_box_area_source_controls_axial_stiffness_and_self_weight() {
    let mut model = brace_model();
    // 外形・内形の直角面積差から、四隅の面積差を差し引く。
    let rounded_area_mm2 =
        SHARP_AREA_MM2 - (4.0 - std::f64::consts::PI) * (40.0_f64.powi(2) - 24.0_f64.powi(2));
    assert_linear_stiffness_and_weight(&model, rounded_area_mm2);

    model.sections[0].area = SHARP_AREA_MM2;
    let analysis = compute_linear_static(model.clone(), LoadCaseId(0));
    let self_weight =
        sepika_load::self_weight::self_weight_case_content(&model, &LoadCfg::default());
    assert!(
        analysis.is_err() && self_weight.is_err(),
        "古い面積が計算に使われました。EA/L={:?} N/mm、自重荷重={:?}",
        analysis
            .as_ref()
            .ok()
            .map(|result| LOAD_N / result.disp[1][0]),
        self_weight.as_ref().ok().map(|(_, member)| member),
    );
    let assert_mismatch = |error: String| {
        assert!(
            error.contains("BOX400")
                && error.contains("24576 mm²")
                && error.contains("一致しません"),
            "{error}"
        );
    };
    assert_mismatch(
        prepare_model_for_analysis(&mut model, &Default::default(), None)
            .err()
            .unwrap()
            .to_string(),
    );
    assert_mismatch(analysis.err().unwrap().to_string());
    assert_mismatch(self_weight.unwrap_err());

    model.sections[0].property_basis.area = PropertyBasis::Supplied;
    assert_linear_stiffness_and_weight(&model, SHARP_AREA_MM2);
}
