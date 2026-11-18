use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId};
use sepika_core::model::{
    ElementData, ElementKind, EndCondition, ForceRegime, Haunch, LocalAxis, Material,
    MaterialCategory, MemberDetailAttr, Model, Node, RigidZone,
};
use sepika_core::section_shape::{
    BeamStirrup, RcBeamRebar, RcRectColumnRebar, RectColumnHoop, SectionShape,
};
use sepika_core::standard_material::standard_material_properties;
use sepika_design_jp::quantity::{try_compute_quantity_takeoff, MemberCategory};

fn model() -> Model {
    let beam = SectionShape::RcBeamRect {
        b: 300.0,
        d: 600.0,
        rebar: RcBeamRebar {
            main_dia: 25.0,
            top: vec![4],
            bottom: vec![4],
            cover: 40.0,
            stirrup: BeamStirrup {
                dia: 10.0,
                pitch: 200.0,
                legs: 2,
            },
        },
    };
    let column = SectionShape::RcColumnRect {
        b: 700.0,
        d: 700.0,
        rebar: RcRectColumnRebar {
            main_dia: 25.0,
            x: vec![4],
            y: vec![4],
            cover: 40.0,
            hoop: RectColumnHoop {
                dia: 13.0,
                pitch: 100.0,
                legs_x: 2,
                legs_y: 2,
            },
        },
    };
    let beam_faces = RigidZone {
        face_i: Some(350.0),
        face_j: Some(350.0),
        ..Default::default()
    };
    Model {
        nodes: [
            [0.0, 0.0, 0.0],
            [6700.0, 0.0, 0.0],
            [0.0, 0.0, 3500.0],
            [6700.0, 0.0, 3500.0],
        ]
        .into_iter()
        .enumerate()
        .map(|(i, coord)| Node {
            id: NodeId(i as u32),
            coord,
            restraint: Default::default(),
            mass: None,
            story: None,
            support_spring: None,
        })
        .collect(),
        elements: [
            ([0, 2], SectionId(1), [1.0, 0.0, 0.0], RigidZone::default()),
            ([1, 3], SectionId(1), [1.0, 0.0, 0.0], RigidZone::default()),
            ([2, 3], SectionId(0), [0.0, 0.0, 1.0], beam_faces),
            ([0, 1], SectionId(0), [0.0, 0.0, 1.0], beam_faces),
        ]
        .into_iter()
        .enumerate()
        .map(
            |(i, (nodes, section, ref_vector, rigid_zone))| ElementData {
                id: ElemId(i as u32),
                kind: ElementKind::Beam,
                nodes: nodes.map(NodeId).into_iter().collect(),
                section: Some(section),
                local_axis: LocalAxis { ref_vector },
                end_cond: [EndCondition::Fixed; 2],
                force_regime: ForceRegime::Auto,
                rigid_zone,
                end_offsets_mm: [[0.0; 3]; 2],
                plastic_zone: None,
                spring: None,
            },
        )
        .collect(),
        sections: [(beam, "G"), (column, "C")]
            .into_iter()
            .enumerate()
            .map(|(i, (shape, name))| {
                let mut section = shape.to_section(SectionId(i as u32), name.into());
                section.frame_use = section.rc_frame_use();
                section.material = Some(MaterialId(0));
                section.rebar_material = Some(MaterialId(1));
                section.shear_rebar_material = Some(MaterialId(1));
                section
            })
            .collect(),
        materials: [
            ("Fc24", MaterialCategory::Concrete),
            ("SD345", MaterialCategory::Rebar),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (name, category))| {
            let properties = standard_material_properties(name).unwrap();
            Material {
                id: MaterialId(i as u32),
                name: name.into(),
                category,
                young: properties.young,
                poisson: properties.poisson,
                density: properties.density,
                shear: None,
                fc: properties.fc,
                fy: properties.fy,
                concrete_class: Default::default(),
                strength_factor: None,
            }
        })
        .collect(),
        ..Default::default()
    }
}

#[test]
fn foundation_formwork_width_increment_counts_twice_the_bottom_projection() {
    let mut model = model();
    let width = Haunch {
        length: 1000.0,
        depth_increase: 0.0,
        width_increase: 100.0,
    };
    let combined = Haunch {
        depth_increase: 200.0,
        ..width
    };
    let another_width = Haunch {
        length: 500.0,
        width_increase: 60.0,
        ..width
    };
    // 6 m の内法で、幅ハンチの投影は i 端が 0.050 m²、j 端の短いハンチが 0.015 m²。
    for (haunch_i, haunch_j, girder_m2, foundation_m2) in [
        (None, None, 9.0, 5.4),
        (Some(width), None, 9.05, 5.5),
        (None, Some(width), 9.05, 5.5),
        (Some(combined), None, 9.25, 5.6),
        (Some(combined), Some(another_width), 9.265, 5.63),
    ] {
        model.member_detail_attrs = [ElemId(2), ElemId(3)]
            .into_iter()
            .map(|elem| MemberDetailAttr {
                elem,
                haunch_i,
                haunch_j,
                joints: vec![],
            })
            .collect();
        let takeoff = try_compute_quantity_takeoff(&model, &model.quantity_cfg).unwrap();
        for (elem, category, expected_m2) in [
            (ElemId(2), MemberCategory::Girder, girder_m2),
            (ElemId(3), MemberCategory::FoundationGirder, foundation_m2),
        ] {
            let item = takeoff
                .items
                .iter()
                .find(|item| item.elem == Some(elem))
                .unwrap();
            assert_eq!(item.category, category);
            assert!(
                (item.formwork_m2 - expected_m2).abs() < 1e-9,
                "{elem:?}: {haunch_i:?}, {haunch_j:?}: {} m²",
                item.formwork_m2
            );
        }
    }
}
