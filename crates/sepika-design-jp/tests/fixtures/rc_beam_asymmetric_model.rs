use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, NodeId, SectionId};
use sepika_core::model::*;
use sepika_core::section_shape::{BeamStirrup, RcBeamRebar, SectionShape};

pub fn material(name: &str, category: MaterialCategory) -> Material {
    let standard = sepika_core::standard_material::standard_material_properties(name).unwrap();
    Material {
        id: MaterialId(if category == MaterialCategory::Concrete {
            0
        } else {
            1
        }),
        name: name.into(),
        category,
        young: standard.young,
        poisson: standard.poisson,
        density: standard.density,
        shear: None,
        fc: standard.fc,
        fy: standard.fy,
        concrete_class: Default::default(),
        strength_factor: None,
    }
}

pub fn section(
    width_mm: f64,
    depth_mm: f64,
    main_dia_mm: f64,
    top: &[u32],
    bottom: &[u32],
) -> Section {
    let shape = SectionShape::RcBeamRect {
        b: width_mm,
        d: depth_mm,
        rebar: RcBeamRebar {
            main_dia: main_dia_mm,
            top: top.to_vec(),
            bottom: bottom.to_vec(),
            cover: 40.0,
            stirrup: BeamStirrup {
                dia: 10.0,
                pitch: 100.0,
                legs: 2,
            },
        },
    };
    shape.validate_rebar().unwrap();
    let mut section = shape.to_section(SectionId(0), "G1".into());
    section.material = Some(MaterialId(0));
    section.rebar_material = Some(MaterialId(1));
    section.shear_rebar_material = Some(MaterialId(1));
    section.frame_use = section.rc_frame_use();
    section
}

pub fn model(asymmetric: bool) -> Model {
    let top: &[u32] = if asymmetric { &[3, 3] } else { &[3] };
    let model = Model {
        nodes: [0.0, 3_000.0]
            .into_iter()
            .enumerate()
            .map(|(i, x)| Node {
                id: NodeId(i as u32),
                coord: [x, 0.0, 0.0],
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
        materials: vec![
            material("FC24", MaterialCategory::Concrete),
            material("SD345", MaterialCategory::Rebar),
        ],
        sections: vec![section(250.0, 350.0, 19.0, top, &[3])],
        elements: vec![ElementData {
            id: ElemId(0),
            kind: ElementKind::Beam,
            nodes: [NodeId(0), NodeId(1)].into_iter().collect(),
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 1.0, 0.0],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            end_offsets_mm: [[0.0; 3]; 2],
            plastic_zone: None,
            spring: None,
        }],
        load_cases: vec![
            LoadCase {
                origin: Default::default(),
                id: LoadCaseId(0),
                name: DL_CASE_NAME.into(),
                kind: LoadCaseKind::Dead,
                nodal: vec![NodalLoad::manual(
                    NodeId(1),
                    [0.0, 0.0, 0.0, 0.0, 0.0, 45e6],
                )],
                member: vec![],
            },
            LoadCase {
                origin: Default::default(),
                id: LoadCaseId(1),
                name: LL_FRAME_CASE_NAME.into(),
                kind: LoadCaseKind::Live,
                nodal: vec![],
                member: vec![],
            },
        ],
        combinations: vec![LoadCombination {
            name: "長期".into(),
            terms: vec![(LoadCaseId(0), 1.0), (LoadCaseId(1), 1.0)],
        }],
        ..Default::default()
    };
    model.validate().unwrap();
    model
}
