use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, NodeId, SectionId, SlabId};
use sepika_core::model::*;

pub fn two_storeys() -> Model {
    let mut model = Model::default();
    for (id, coord) in [
        [0.0, 0.0, 0.0],
        [6000.0, 0.0, 0.0],
        [0.0, 0.0, 12500.0],
        [6000.0, 0.0, 12500.0],
        [0.0, 0.0, 25000.0],
        [6000.0, 0.0, 25000.0],
    ]
    .into_iter()
    .enumerate()
    {
        model.nodes.push(Node {
            id: NodeId(id as u32),
            coord,
            restraint: if id < 2 {
                Dof6Mask::FIXED
            } else {
                Dof6Mask::FREE
            },
            mass: None,
            story: None,
            support_spring: None,
        });
    }
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
    let mut slab_material = model.materials[0].clone();
    slab_material.id = MaterialId(1);
    model.materials.push(slab_material);
    model.materials.push(Material {
        id: MaterialId(2),
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
    });
    use sepika_core::section_shape::{
        BeamStirrup, RcBeamRebar, RcRectColumnRebar, RectColumnHoop, SectionShape,
    };
    let mut section = SectionShape::RcColumnRect {
        b: 300.0,
        d: 300.0,
        rebar: RcRectColumnRebar {
            main_dia: 22.0,
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
    .to_section(SectionId(0), "RC柱".into());
    section.material = Some(MaterialId(0));
    section.rebar_material = Some(MaterialId(2));
    section.shear_rebar_material = Some(MaterialId(2));
    section.frame_use = Some(FrameSectionUse::Column);
    model.sections.push(section);
    let mut section = SectionShape::RcBeamRect {
        b: 300.0,
        d: 400.0,
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
    .to_section(SectionId(1), "RC梁".into());
    section.material = Some(MaterialId(0));
    section.rebar_material = Some(MaterialId(2));
    section.shear_rebar_material = Some(MaterialId(2));
    section.frame_use = Some(FrameSectionUse::Girder);
    model.sections.push(section);
    let mut slab_section = sepika_core::section_shape::SectionShape::RcSlab { thickness: 100.0 }
        .to_section(SectionId(2), "床版".into());
    slab_section.material = Some(MaterialId(1));
    model.sections.push(slab_section);
    for (id, (i, j, section)) in [
        (0, 2, 0),
        (1, 3, 0),
        (2, 4, 0),
        (3, 5, 0),
        (2, 3, 1),
        (4, 5, 1),
    ]
    .into_iter()
    .enumerate()
    {
        model.elements.push(ElementData {
            id: ElemId(id as u32),
            kind: ElementKind::Beam,
            nodes: [NodeId(i), NodeId(j)].into_iter().collect(),
            section: Some(SectionId(section)),
            local_axis: LocalAxis {
                ref_vector: if section == 0 {
                    [1.0, 0.0, 0.0]
                } else {
                    [0.0, 0.0, 1.0]
                },
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            plastic_zone: None,
            spring: None,
        });
    }
    model.load_cases.push(LoadCase {
        id: LoadCaseId(0),
        name: DL_CASE_NAME.into(),
        kind: LoadCaseKind::Dead,
        nodal: [2, 3, 4, 5]
            .into_iter()
            .map(|id| {
                let frame_volume = if id < 4 { 2934000000.0 } else { 1809000000.0 };
                let self_weight =
                    (frame_volume + 1200000000.0) * 2.4e-9 * sepika_core::units::GRAVITY_MM_S2;
                NodalLoad::manual(
                    NodeId(id),
                    [0.0, 0.0, -(100000.0 - self_weight) / 2.0, 0.0, 0.0, 0.0],
                )
            })
            .collect(),
        member: Vec::new(),
    });
    for (id, nodes) in [[NodeId(2), NodeId(3)], [NodeId(4), NodeId(5)]]
        .into_iter()
        .enumerate()
    {
        model.slabs.push(Slab {
            id: SlabId(id as u32),
            shape: SlabShape::Attached {
                anchor: RegionAnchor::Line {
                    nodes,
                    span: [0.0, 1.0],
                    transfer: LoadTransfer::Anchor,
                },
                extent: [2000.0, 2000.0],
            },
            plate: SlabPlate {
                section: Some(SectionId(2)),
                loads: Vec::new(),
                usage: None,
                method: DistributionMethod::TriTrapezoid,
                one_way: None,
            },
            tip_loads: Vec::new(),
        });
    }
    model
}

pub fn expected_weights(density: f64, finish: f64, seismic_live: f64) -> [f64; 2] {
    let gamma = (density - 2.4e-9) * sepika_core::units::GRAVITY_MM_S2;
    [
        100000.0 + 2934000000.0 * gamma + 12000000.0 * (finish + seismic_live),
        100000.0 + 1809000000.0 * gamma + 12000000.0 * (finish + seismic_live),
    ]
}
