use sepika_core::dof::{Dof, Dof6Mask};
use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, NodeId, SectionId};
use sepika_core::model::*;
use sepika_core::section_shape::SectionShape;

pub const BRACE: ElemId = ElemId(0);

pub fn model() -> Model {
    let mut section = SectionShape::SteelFlatBar {
        width: 75.0,
        thick: 10.0,
    }
    .to_section(SectionId(0), "平鋼 75×10".into());
    section.material = Some(MaterialId(0));
    section.frame_use = Some(FrameSectionUse::Brace);

    let mut fixed_except_x = Dof6Mask::FIXED;
    fixed_except_x.set_free(Dof::Ux);
    Model {
        nodes: vec![
            Node {
                id: NodeId(0),
                coord: [0.0; 3],
                restraint: Dof6Mask::FIXED,
                mass: None,
                story: None,
                support_spring: None,
            },
            Node {
                id: NodeId(1),
                coord: [5000.0, 0.0, 0.0],
                restraint: fixed_except_x,
                mass: None,
                story: None,
                support_spring: None,
            },
        ],
        materials: vec![Material {
            id: MaterialId(0),
            name: "直接鋼材".into(),
            category: MaterialCategory::Steel,
            young: 205_000.0,
            poisson: 0.3,
            density: 0.0,
            shear: None,
            fc: None,
            fy: Some(235.0),
            strength_factor: None,
            concrete_class: Default::default(),
        }],
        sections: vec![section],
        elements: vec![ElementData {
            id: BRACE,
            kind: ElementKind::Brace {
                tension_only: false,
            },
            nodes: [NodeId(0), NodeId(1)].into_iter().collect(),
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 1.0, 0.0],
            },
            end_cond: [EndCondition::Pinned; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            end_offsets_mm: [[0.0; 3]; 2],
            plastic_zone: None,
            spring: None,
        }],
        brb_attrs: vec![BrbAttr {
            elem: BRACE,
            allowable_axial_short: Some(1_000_000.0),
            critical_length: Some(4400.0),
            length_reduction: Some(300.0),
        }],
        load_cases: vec![LoadCase {
            origin: Default::default(),
            id: LoadCaseId(0),
            name: "ブレースの軸力".into(),
            kind: LoadCaseKind::Dead,
            nodal: vec![NodalLoad::manual(
                NodeId(1),
                [-500_000.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            )],
            member: vec![],
        }],
        panel_zone: PanelZoneMode::None,
        ..Default::default()
    }
}

pub fn pipe_with_brb_attr(frame_use: FrameSectionUse) -> Model {
    let (end_mm, moment_nmm) = match frame_use {
        FrameSectionUse::Column => ([0.0, 0.0, 3000.0], [-100e6, 0.0, 0.0]),
        FrameSectionUse::Girder => ([3000.0, 0.0, 0.0], [0.0, 0.0, 100e6]),
        FrameSectionUse::Brace => panic!("柱または梁の断面用途が必要です"),
    };
    let mut model = model();
    let mut section = SectionShape::SteelPipe {
        outer_dia: 200.0,
        thick: 10.0,
    }
    .to_section(SectionId(0), "円形鋼管 D200×10".into());
    section.material = Some(MaterialId(0));
    section.frame_use = Some(frame_use);
    model.sections[0] = section;
    model.nodes[1].coord = end_mm;
    model.nodes[1].restraint = Dof6Mask::FREE;
    model.elements[0].kind = ElementKind::Beam;
    model.elements[0].end_cond = [EndCondition::Fixed; 2];
    model.brb_attrs[0].allowable_axial_short = Some(100_000.0);
    model.brb_attrs[0].critical_length = Some(6000.0);
    model.brb_attrs[0].length_reduction = Some(0.0);
    model.load_cases[0].name = "片持ち部材の曲げ".into();
    model.load_cases[0].nodal = vec![NodalLoad::manual(
        NodeId(1),
        [0.0, 0.0, 0.0, moment_nmm[0], moment_nmm[1], moment_nmm[2]],
    )];
    model
}
