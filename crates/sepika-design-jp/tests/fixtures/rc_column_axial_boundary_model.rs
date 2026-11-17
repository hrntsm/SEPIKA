use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, NodeId, SectionId};
use sepika_core::model::*;
use sepika_core::section_shape::*;
pub fn column_model(circle: bool, axial_load_n: f64) -> Model {
    let shape = if circle {
        SectionShape::RcColumnCircle {
            d: 600.0,
            rebar: RcCircleColumnRebar {
                main_dia: 19.0,
                count: 8,
                cover: 40.0,
                hoop: CircleColumnHoop {
                    dia: 10.0,
                    pitch: 100.0,
                },
            },
        }
    } else {
        SectionShape::RcColumnRect {
            b: 400.0,
            d: 600.0,
            rebar: RcRectColumnRebar {
                main_dia: 19.0,
                x: vec![3],
                y: vec![3],
                cover: 40.0,
                hoop: RectColumnHoop {
                    dia: 10.0,
                    pitch: 100.0,
                    legs_x: 2,
                    legs_y: 2,
                },
            },
        }
    };
    shape.validate_rebar().unwrap();
    let mut section = shape.to_section(SectionId(0), "許容軸力境界のRC柱".into());
    section.material = Some(MaterialId(0));
    section.rebar_material = Some(MaterialId(1));
    section.shear_rebar_material = Some(MaterialId(1));
    section.frame_use = Some(FrameSectionUse::Column);
    let mut model = Model::default();
    model.sections.push(section);
    model.materials = vec![
        Material {
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
        },
        Material {
            id: MaterialId(1),
            name: "SD345".into(),
            category: MaterialCategory::Rebar,
            young: 205000.0,
            poisson: 0.3,
            density: 7.85e-9,
            shear: None,
            fc: None,
            fy: Some(345.0),
            concrete_class: Default::default(),
            strength_factor: None,
        },
    ];
    for (id, z) in [(0, 0.0), (1, 3000.0)] {
        model.nodes.push(Node {
            id: NodeId(id),
            coord: [0.0, 0.0, z],
            restraint: if id == 0 {
                Dof6Mask::FIXED
            } else {
                Dof6Mask::FREE
            },
            mass: None,
            story: None,
            support_spring: None,
        });
    }
    model.elements.push(ElementData {
        id: ElemId(0),
        kind: ElementKind::Beam,
        nodes: [NodeId(0), NodeId(1)].into_iter().collect(),
        section: Some(SectionId(0)),
        local_axis: LocalAxis {
            ref_vector: [1.0, 0.0, 0.0],
        },
        end_cond: [EndCondition::Fixed; 2],
        force_regime: ForceRegime::Auto,
        rigid_zone: Default::default(),
        end_offsets_mm: [[0.0; 3]; 2],
        plastic_zone: None,
        spring: None,
    });
    model.load_cases.push(LoadCase {
        origin: Default::default(),
        id: LoadCaseId(0),
        name: DL_CASE_NAME.into(),
        kind: LoadCaseKind::Dead,
        nodal: vec![NodalLoad::manual(
            NodeId(1),
            [0.0, 0.0, -axial_load_n, 1_000_000.0, 2_000_000.0, 0.0],
        )],
        member: vec![],
    });
    model.stories = [0.0, 3000.0]
        .into_iter()
        .enumerate()
        .map(|(id, fl)| {
            Story::new(
                sepika_core::ids::StoryId(id as u32),
                format!("{}F", id + 1),
                Some(fl),
                Some(0.0),
            )
        })
        .collect();
    model.validate().unwrap();
    model
}
