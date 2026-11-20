use sepika_core::ids::{ElemId, NodeId, StoryId};
use sepika_core::model::{Constraint, Model};
use sepika_core::units::GRAVITY_MM_S2;
use sepika_element::{frame::beam::MemberForces, transform::LocalFrame};
use sepika_solver::dynamic::lumped_mass::bending_drifts;
use sepika_solver::statics::{analysis::SeismicDir, linear::StaticOnce};

pub fn two_story_column_model() -> Model {
    use sepika_core::dof::Dof6Mask;
    use sepika_core::ids::{ElemId, NodeId, StoryId};
    use sepika_core::model::{
        Constraint, ElementData, ElementKind, EndCondition, ForceRegime, FrameSectionUse,
        LocalAxis, Node, Section, Story,
    };
    let mut model = Model::default();
    let coords = [[0.0, 0.0, 0.0], [0.0, 0.0, 3000.0], [0.0, 0.0, 6000.0]];
    for (i, c) in coords.iter().enumerate() {
        model.nodes.push(Node {
            id: NodeId(i as u32),
            coord: *c,
            restraint: if i == 0 {
                Dof6Mask::FIXED
            } else {
                Dof6Mask::FREE
            },
            mass: None,
            story: Some(StoryId(i as u32)),
            support_spring: None,
        });
    }
    for i in 0..3u32 {
        model.stories.push(Story {
            wall_weights: Vec::new(),
            id: StoryId(i),
            name: format!("{}F", i + 1),
            elevation: Some(coords[i as usize][2]),
            sl_offset_mm: Some(0.0),
            source_story: None,
            node_ids: vec![NodeId(i)],
            seismic_weight: if i == 0 {
                None
            } else {
                Some(f64::from(3 - i) * 10.0 * GRAVITY_MM_S2)
            },
            weight_override: None,
            structure: Default::default(),
            level_kind: Default::default(),
            dynamic_mass: Some(sepika_core::model::StoryDynamicMass {
                horizontal_inertia_t_mm2: [0.0; 3],
                mass_equiv_weight_n: f64::from(3 - i) * 10.0 * GRAVITY_MM_S2,
                lumped_mass: None,
                center_xy_mm: [f64::from(i), 0.0],
                inertia_t_mm2: 1000.0,
            }),
            standard_floor_load: None,
            column_finish_area_weight: 0.0,
            fireproof: Default::default(),
        });
    }
    model.sections.push(Section {
        frame_use: Some(FrameSectionUse::Column),
        id: sepika_core::ids::SectionId(0),
        name: "column".into(),
        area: 1.0,
        iy: 1.0,
        iz: 1.0,
        j: 1.0,
        depth: 1.0,
        width: 1.0,
        as_y: 1.0,
        as_z: 1.0,
        floor: None,
        panel_thickness: None,
        thickness: None,
        shape: None,
        material: None,
        rebar_material: None,
        shear_rebar_material: None,
        steel_material: None,
        property_basis: Default::default(),
    });
    for i in 0..2u32 {
        model.elements.push(ElementData {
            id: ElemId(i),
            kind: ElementKind::Beam,
            nodes: [NodeId(i), NodeId(i + 1)].into_iter().collect(),
            section: Some(sepika_core::ids::SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 1.0, 0.0],
            },
            end_cond: [EndCondition::Fixed, EndCondition::Fixed],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            end_offsets_mm: [[0.0, 0.0, 0.0]; 2],
            plastic_zone: None,
            spring: None,
        });
    }
    for i in 0..3u32 {
        model.constraints.push(Constraint::RigidDiaphragm {
            story: StoryId(i),
            master: NodeId(i),
            slaves: vec![],
            weight: None,
            ci_override: None,
        });
    }
    model
}

pub fn bending_shear_response(dir: SeismicDir, reverse: bool, split: bool) -> (Model, StaticOnce) {
    let mut model = two_story_column_model();
    let node = model.nodes[0].clone();
    let element = model.elements[0].clone();
    model.nodes.clear();
    model.elements.clear();
    model.constraints.clear();
    let xy = [
        [-1000.0, -2000.0],
        [1000.0, -2000.0],
        [-1000.0, 2000.0],
        [1000.0, 2000.0],
    ];
    for level in 0..3 {
        let ids: Vec<_> = (0..4).map(|i| NodeId((level * 4 + i) as u32)).collect();
        for (k, &[x, y]) in xy.iter().enumerate() {
            let mut n = node.clone();
            n.id = ids[k];
            n.coord = [x, y, level as f64 * 3000.0];
            n.story = Some(StoryId(level as u32));
            n.restraint = if level == 0 {
                sepika_core::dof::Dof6Mask::FIXED
            } else {
                sepika_core::dof::Dof6Mask::FREE
            };
            model.nodes.push(n);
        }
        model.stories[level].node_ids = ids.clone();
        let dm = model.stories[level].dynamic_mass.as_mut().unwrap();
        dm.center_xy_mm = [0.0, 0.0];
        dm.horizontal_inertia_t_mm2 = [4e7, 1e7, 0.0];
        dm.inertia_t_mm2 = 5e7;
        model.constraints.push(Constraint::RigidDiaphragm {
            story: StoryId(level as u32),
            master: ids[0],
            slaves: ids[1..].to_vec(),
            weight: None,
            ci_override: None,
        });
    }
    for layer in 0..2 {
        for (k, point) in xy.iter().enumerate() {
            let bottom = NodeId((layer * 4 + k) as u32);
            let top = NodeId(((layer + 1) * 4 + k) as u32);
            let segments = if split {
                let mut n = node.clone();
                n.id = NodeId(model.nodes.len() as u32);
                n.coord = [point[0], point[1], layer as f64 * 3000.0 + 1500.0];
                n.story = Some(StoryId(layer as u32));
                n.restraint = sepika_core::dof::Dof6Mask::FREE;
                let mid = n.id;
                model.nodes.push(n);
                vec![[bottom, mid], [mid, top]]
            } else {
                vec![[bottom, top]]
            };
            for mut pair in segments {
                if reverse {
                    pair.reverse();
                }
                let mut e = element.clone();
                e.id = ElemId(model.elements.len() as u32);
                e.nodes = pair.into_iter().collect();
                model.elements.push(e);
            }
        }
    }
    let axis = usize::from(dir == SeismicDir::Y);
    let ei = if axis == 0 { 1.2e14 } else { 4.8e14 };
    let db = bending_drifts(&[3000.0; 2], &[ei; 2], &[2000.0, 1000.0]).unwrap();
    let mut disp = vec![[0.0; 6]; model.nodes.len()];
    for n in &model.nodes {
        let level = n.coord[2] / 3000.0;
        let moments = [6e6, 3e6];
        let mut theta = 0.0;
        let full = level.floor() as usize;
        for &m in moments.iter().take(full) {
            theta += 3000.0 * m / ei;
        }
        if full < 2 {
            theta += (level - full as f64) * 3000.0 * moments[full] / ei;
        }
        disp[n.id.index()][2] = -theta * n.coord[axis];
        if level == 1.0 {
            disp[n.id.index()][axis] = db[0] + 0.2;
        }
        if level == 2.0 {
            disp[n.id.index()][axis] = db[0] + db[1] + 0.3;
        }
    }
    let mut member_forces = Vec::new();
    let mut column_end_horizontal_forces = Vec::new();
    for e in &model.elements {
        let p0 = model.nodes[e.nodes[0].index()].coord;
        let p1 = model.nodes[e.nodes[1].index()].coord;
        let layer = ((p0[2] + p1[2]) / 2.0 / 3000.0) as usize;
        let n = -[6e6, 3e6][layer] * p0[axis] / if axis == 0 { 4e6 } else { 16e6 };
        let frame = LocalFrame::from_nodes(p0, p1, e.local_axis.ref_vector).unwrap();
        let q = [2000.0, 1000.0][layer] / 4.0;
        let mut lower = [0.0; 2];
        lower[axis] = -q;
        let upper = lower.map(|value| -value);
        let ends = if reverse {
            [upper, lower]
        } else {
            [lower, upper]
        };
        column_end_horizontal_forces.push((e.id, Ok(ends)));
        let local = [
            n,
            -q * frame.rot[0][2].signum() * frame.rot[1][axis],
            -q * frame.rot[0][2].signum() * frame.rot[2][axis],
            0.0,
            0.0,
            0.0,
        ];
        member_forces.push((
            e.id,
            MemberForces {
                at: vec![(0.0, local), (1.0, local)],
            },
        ));
    }
    (
        model,
        StaticOnce {
            story_shears: Some([vec![2000.0, 1000.0], vec![2000.0, 1000.0]]),
            stiffness_ratio_disp: None,
            story_rigidity: Vec::new(),
            disp,
            member_forces,
            panel_moments: Vec::new(),
            girder_column_face_forces: Vec::new(),
            column_end_axial_forces: Vec::new(),
            column_end_horizontal_forces,
        },
    )
}
