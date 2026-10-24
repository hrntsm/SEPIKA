use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, NodeId};
use sepika_core::model::{
    ElementData, ElementKind, EndCondition, ForceRegime, LocalAxis, Model, Node,
};
use sepika_design_jp::{run_member_design_checks, MemberDesignCheckOptions};

#[test]
fn whole_model_design_entry_rejects_invalid_local_reference_with_member_id() {
    let mut model = Model {
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
        section: None,
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
