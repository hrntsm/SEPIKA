use sepika_core::dof::{Dof, Dof6Mask};
use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, NodeId, SectionId, StoryId};
use sepika_core::model::{
    Constraint, ElementData, ElementKind, EndCondition, ForceRegime, FrameSectionUse,
    GirderTorsionMode, LoadCase, LoadCaseKind, LocalAxis, Material, MaterialCategory, MemberLoad,
    Model, NodalLoad, Node, RigidZone, Section, Story,
};
use sepika_element::factory::build_behavior;
use sepika_solver::statics::linear::StaticOnce;

pub const TOP: StoryId = StoryId(1);

pub fn three_column_rigid_floor([dx_mm, dy_mm]: [f64; 2]) -> Model {
    let mut model = column_model(&[[0.0, 0.0], [6000.0, 0.0], [0.0, 6000.0]], dx_mm, dy_mm);
    if !sepika_core::geom::is_vertical_axis(model.nodes[0].coord, model.nodes[1].coord) {
        // 通常解析は鉛直より水平に近い材の柱用断面指定を受け付けない。
        let mut section = model.sections[0].clone();
        section.id = SectionId(1);
        section.name = "傾斜部材断面".into();
        section.frame_use = Some(FrameSectionUse::Girder);
        model.sections.push(section);
        model.elements[0].section = Some(SectionId(1));
        model.girder_torsion = GirderTorsionMode::Keep;
    }
    for head in model.nodes.iter_mut().skip(1).step_by(2) {
        for dof in [Dof::Uz, Dof::Rx, Dof::Ry] {
            head.restraint.set_fixed(dof);
        }
    }
    model.constraints.push(Constraint::rigid_diaphragm(
        TOP,
        NodeId(1),
        vec![NodeId(3), NodeId(5)],
    ));
    model
}

pub fn oppositely_rotated_columns_rigid_floor() -> Model {
    let mut model = three_column_rigid_floor([0.0; 2]);
    model.sections[0].j = 0.0;
    let mut section = model.sections[0].clone();
    section.id = SectionId(1);
    section.name = "方向別剛性".into();
    section.iz = 16_000_000.0;
    model.sections.push(section);
    for (element, local_y) in model
        .elements
        .iter_mut()
        .zip([[1.0, 1.0, 0.0], [1.0, -1.0, 0.0]])
    {
        element.section = Some(SectionId(1));
        element.local_axis.ref_vector = local_y;
    }
    model
}

pub fn column_model(heads_mm: &[[f64; 2]], first_dx_mm: f64, first_dy_mm: f64) -> Model {
    let mut model = Model {
        stories: vec![
            Story::new(StoryId(0), "基礎".into(), Some(0.0), Some(0.0)),
            Story::new(TOP, "上階".into(), Some(3000.0), Some(0.0)),
        ],
        materials: vec![Material {
            id: MaterialId(0),
            name: "鋼".into(),
            category: MaterialCategory::Steel,
            young: 200_000.0,
            poisson: 0.25,
            density: 0.0,
            shear: None,
            fc: None,
            fy: None,
            strength_factor: None,
            concrete_class: Default::default(),
        }],
        ..Default::default()
    };
    let mut section = Section::zero(SectionId(0), "柱断面".into());
    section.frame_use = Some(FrameSectionUse::Column);
    section.material = Some(MaterialId(0));
    section.area = 2_000.0;
    section.as_y = 1_600.0;
    section.as_z = 1_600.0;
    section.iy = 8_000_000.0;
    section.iz = 8_000_000.0;
    section.j = 4_000_000.0;
    model.sections.push(section);
    for (index, &[x, y]) in heads_mm.iter().enumerate() {
        let bottom = NodeId((2 * index) as u32);
        let top = NodeId(bottom.0 + 1);
        let base_xy = if index == 0 {
            [x - first_dx_mm, y - first_dy_mm]
        } else {
            [x, y]
        };
        for (id, coord, restraint, story) in [
            (
                bottom,
                [base_xy[0], base_xy[1], 0.0],
                Dof6Mask::FIXED,
                StoryId(0),
            ),
            (top, [x, y, 3000.0], Dof6Mask::FREE, TOP),
        ] {
            model.nodes.push(Node {
                id,
                coord,
                restraint,
                mass: None,
                story: Some(story),
                support_spring: None,
            });
            model.stories[story.index()].node_ids.push(id);
        }
        model.elements.push(ElementData {
            id: ElemId(index as u32),
            kind: ElementKind::Beam,
            nodes: [bottom, top].into_iter().collect(),
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 1.0, 0.0],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: RigidZone::default(),
            end_offsets_mm: [[0.0; 3]; 2],
            plastic_zone: None,
            spring: None,
        });
    }
    model
}

pub fn add_case(model: &mut Model, name: &str, values: [f64; 6]) -> LoadCaseId {
    let id = LoadCaseId(model.load_cases.len() as u32);
    model.load_cases.push(LoadCase {
        origin: Default::default(),
        id,
        name: name.into(),
        kind: LoadCaseKind::Seismic,
        nodal: vec![NodalLoad::manual(NodeId(1), values)],
        member: Vec::new(),
    });
    id
}

pub fn element_nodal_residual(
    model: &Model,
    element: &ElementData,
    result: &StaticOnce,
    loads: &[MemberLoad],
) -> [f64; 12] {
    let mut disp = [0.0; 12];
    for (dofs, node) in disp.as_chunks_mut::<6>().0.iter_mut().zip(&element.nodes) {
        dofs.copy_from_slice(&result.disp[node.index()]);
    }
    build_behavior(element, model)
        .frame_load_response(&disp, loads)
        .unwrap()
        .unwrap()
        .nodal_residual
        .data
        .as_slice()
        .try_into()
        .unwrap()
}
