use sepika_core::dof::{Dof, Dof6Mask};
use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, NodeId, SectionId};
use sepika_core::model::*;

pub fn portal(beam_i_ratio: f64) -> Model {
    let mut model = Model::default();
    model.materials.push(Material {
        id: MaterialId(0),
        name: "直接鋼材".into(),
        category: MaterialCategory::Steel,
        young: 205000.0,
        poisson: 0.3,
        density: 7.85e-9,
        shear: None,
        fc: None,
        fy: Some(235.0),
        strength_factor: None,
        concrete_class: Default::default(),
    });
    for (id, usage, i_ratio) in [
        (0, FrameSectionUse::Column, 1.0),
        (1, FrameSectionUse::Girder, beam_i_ratio),
    ] {
        model.sections.push(Section {
            material: Some(MaterialId(0)),
            frame_use: Some(usage),
            area: 15600.0,
            as_y: 7600.0,
            as_z: 7600.0,
            iy: 395720000.0 * i_ratio,
            iz: 395720000.0 * i_ratio,
            j: 593190000.0,
            depth: 400.0,
            width: 400.0,
            shape: Some(sepika_core::section_shape::SectionShape::SteelBox {
                height: 400.0,
                width: 400.0,
                thick: 10.0,
                horizontal_thick: 10.0,
                corner_r: Some(0.0),
            }),
            ..Section::zero(SectionId(id), "直接性能".into())
        });
    }
    for (id, coord) in [
        (0, [0.0, 0.0, 0.0]),
        (1, [3000.0, 0.0, 0.0]),
        (2, [0.0, 0.0, 3000.0]),
        (3, [3000.0, 0.0, 3000.0]),
    ] {
        let mut restraint = if id < 2 {
            Dof6Mask::PINNED
        } else {
            Dof6Mask::FREE
        };
        for dof in [Dof::Uy, Dof::Rx, Dof::Rz] {
            restraint.set_fixed(dof);
        }
        model.nodes.push(Node {
            id: NodeId(id),
            coord,
            restraint,
            mass: None,
            story: None,
            support_spring: None,
        });
    }
    for (id, ends) in [(0, [0, 2]), (1, [1, 3]), (2, [2, 3])] {
        model.elements.push(ElementData {
            id: ElemId(id),
            kind: ElementKind::Beam,
            nodes: ends.into_iter().map(NodeId).collect(),
            section: Some(SectionId(if id == 2 { 1 } else { 0 })),
            local_axis: LocalAxis {
                ref_vector: if id == 2 {
                    [0.0, 0.0, 1.0]
                } else {
                    [1.0, 0.0, 0.0]
                },
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            end_offsets_mm: [[0.0; 3]; 2],
            plastic_zone: None,
            spring: None,
        });
    }
    model.load_cases.push(LoadCase {
        origin: Default::default(),
        id: LoadCaseId(0),
        name: "柱頭鉛直荷重".into(),
        kind: LoadCaseKind::Dead,
        nodal: [2, 3]
            .into_iter()
            .map(|id| NodalLoad::manual(NodeId(id), [0.0, 0.0, -2215200.0, 0.0, 0.0, 0.0]))
            .collect(),
        member: vec![],
    });
    for id in 0..2 {
        model.steel_design_attrs.push(SteelDesignAttr {
            elem: ElemId(id),
            joint_flange_loss: 0.0,
            joint_web_loss: 0.0,
            scallop_web_loss: 0.0,
            lb_direct: None,
            lateral_brace_count: None,
            lk_y_direct: None,
            lk_z_direct: Some(3000.0),
            c_direct: None,
        });
    }
    model
}
