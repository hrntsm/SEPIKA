use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId};
use sepika_core::model::{
    ElementData, ElementKind, EndCondition, ForceRegime, FrameSectionUse, LocalAxis, Material,
    MaterialCategory, Model, Node,
};
use sepika_core::section_shape::SectionShape;
use sepika_design_jp::{run_member_design_checks, MemberDesignCheckOptions};

#[test]
fn whole_model_design_entry_rejects_invalid_local_reference_with_member_id() {
    let mut section = SectionShape::SteelH {
        height: 400.0,
        width: 200.0,
        web_thick: 9.0,
        flange_thick: 12.0,
        root_r: None,
    }
    .input_section(SectionId(0), "柱".into())
    .unwrap();
    section.frame_use = Some(FrameSectionUse::Column);
    section.material = Some(MaterialId(0));
    let mut model = Model {
        sections: vec![section],
        materials: vec![Material {
            id: MaterialId(0),
            name: "鋼材".into(),
            category: MaterialCategory::Steel,
            young: 205000.0,
            poisson: 0.3,
            density: 0.0,
            shear: None,
            fc: None,
            fy: Some(235.0),
            concrete_class: Default::default(),
            strength_factor: None,
        }],
        nodes: (0..2)
            .map(|id| Node {
                id: NodeId(id),
                coord: [0.0, 0.0, f64::from(id) * 3000.0],
                restraint: Dof6Mask::FREE,
                mass: None,
                story: None,
                support_spring: None,
            })
            .collect(),
        ..Default::default()
    };
    model.elements.push(ElementData {
        id: ElemId(42),
        kind: ElementKind::Beam,
        nodes: [NodeId(0), NodeId(1)].into_iter().collect(),
        section: Some(SectionId(0)),
        local_axis: LocalAxis {
            ref_vector: [f64::NAN, 0.0, 0.0],
        },
        end_cond: [EndCondition::Fixed; 2],
        force_regime: ForceRegime::Auto,
        rigid_zone: Default::default(),
        vertical_eccentricity: Some(0.0),
        plastic_zone: None,
        spring: None,
    });
    let error = run_member_design_checks(&model, &[], &[], &MemberDesignCheckOptions::default())
        .unwrap_err();
    assert!(error.contains("部材 ID 42"), "{error}");
    assert!(error.contains("非有限"), "{error}");
}
