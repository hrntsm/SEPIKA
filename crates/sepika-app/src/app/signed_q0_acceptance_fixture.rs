use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, NodeId, SectionId};
use sepika_core::model::*;
use sepika_core::section_shape::{BeamStirrup, RcBeamRebar, SectionShape};
pub fn model() -> Model {
    let mut model = Model {
        nodes: [0.0, 6000.0]
            .into_iter()
            .enumerate()
            .map(|(i, x)| Node {
                id: NodeId(i as u32),
                coord: [x, 0.0, 0.0],
                restraint: Default::default(),
                mass: None,
                story: None,
                support_spring: None,
            })
            .collect(),
        ..Default::default()
    };
    model.materials.push(Material {
        id: MaterialId(0),
        name: "Fc24".into(),
        category: MaterialCategory::Concrete,
        young: 23000.0,
        poisson: 0.2,
        density: 2.4e-9,
        shear: None,
        fc: Some(24.0),
        fy: None,
        concrete_class: Default::default(),
        strength_factor: None,
    });
    let mut section = SectionShape::RcBeamRect {
        b: 300.0,
        d: 600.0,
        rebar: RcBeamRebar {
            main_dia: 22.0,
            top: vec![3],
            bottom: vec![3],
            cover: 40.0,
            stirrup: BeamStirrup {
                dia: 10.0,
                pitch: 100.0,
                legs: 2,
            },
        },
    }
    .to_section(SectionId(0), "RC梁".into());
    section.material = Some(MaterialId(0));
    model.sections.push(section);
    model.elements.push(ElementData {
        id: ElemId(0),
        kind: ElementKind::Beam,
        nodes: [NodeId(0), NodeId(1)].into_iter().collect(),
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
    });
    model.load_cases = [LoadCaseKind::Dead, LoadCaseKind::Live]
        .into_iter()
        .enumerate()
        .map(|(i, kind)| LoadCase {
            id: LoadCaseId(i as u32),
            name: "任意名称".into(),
            kind,
            nodal: vec![],
            member: vec![],
        })
        .collect();
    model.nodes[0].restraint = sepika_core::dof::Dof6Mask::FIXED;
    model.nodes[1].restraint = sepika_core::dof::Dof6Mask::FREE;
    model.sections[0].frame_use = Some(FrameSectionUse::Girder);
    model.sections[0].rebar_material = Some(MaterialId(1));
    model.sections[0].shear_rebar_material = Some(MaterialId(1));
    let mut rebar = model.materials[0].clone();
    rebar.id = MaterialId(1);
    rebar.name = "SD345".into();
    rebar.category = MaterialCategory::Rebar;
    rebar.fc = None;
    rebar.fy = Some(345.0);
    model.materials.push(rebar);
    model.load_cases[0].member = vec![point(1500.0, 1000.0)];
    model.load_cases[1].member = vec![point(4500.0, 1000.0)];
    model.load_cases.push(LoadCase {
        id: LoadCaseId(2),
        name: "独立地震".into(),
        kind: LoadCaseKind::Seismic,
        member: vec![],
        nodal: vec![NodalLoad::manual(
            NodeId(1),
            [0.0, 0.0, -10000.0, 0.0, 0.0, 0.0],
        )],
    });
    model.combinations = vec![
        LoadCombination {
            name: "独立選択".into(),
            terms: vec![
                (LoadCaseId(0), 1.0),
                (LoadCaseId(1), 1.0),
                (LoadCaseId(2), 1.0),
            ],
        },
        LoadCombination {
            name: "独立重力".into(),
            terms: vec![(LoadCaseId(0), 1.0), (LoadCaseId(1), 1.0)],
        },
    ];
    model
}

pub fn point(a: f64, p: f64) -> MemberLoad {
    MemberLoad::manual(ElemId(0), [0.0, 0.0, -1.0], MemberLoadKind::Point { a, p })
}
