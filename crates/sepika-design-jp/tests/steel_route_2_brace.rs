use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId};
use sepika_core::model::{
    ElementData, ElementKind, EndCondition, ForceRegime, FrameSectionUse, LocalAxis, Material,
    MaterialCategory, Model, Node,
};
use sepika_core::section_shape::SectionShape;
use sepika_design_jp::{
    run_member_design_checks, CheckKind, CheckOutcome, LoadTerm, MemberDesignCheckOptions,
};
use sepika_element::frame::beam::MemberForces;

#[test]
fn unamplified_route_2_brace_demand_can_pass_when_amplified_demand_fails() {
    let elem_id = ElemId(0);
    let mut section = SectionShape::SteelFlatBar {
        width: 75.0,
        thick: 10.0,
    }
    .to_section(SectionId(0), "FB-75x10".into());
    section.material = Some(MaterialId(0));
    section.frame_use = Some(FrameSectionUse::Brace);
    let model = Model {
        sections: vec![section],
        materials: vec![Material {
            id: MaterialId(0),
            name: "基準強度235 N/mm²".into(),
            category: MaterialCategory::Steel,
            young: 205_000.0,
            poisson: 0.3,
            density: 7.85e-9,
            shear: None,
            fc: None,
            fy: Some(235.0),
            strength_factor: None,
            concrete_class: Default::default(),
        }],
        nodes: [[0.0; 3], [400.0, 0.0, 300.0]]
            .into_iter()
            .enumerate()
            .map(|(i, coord)| Node {
                id: NodeId(i as u32),
                coord,
                restraint: Dof6Mask::FREE,
                mass: None,
                story: None,
                support_spring: None,
            })
            .collect(),
        elements: vec![ElementData {
            id: elem_id,
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
        ..Default::default()
    };

    // 長期50 kNと地震100 kNを組み合わせる。β=0.5では地震分だけ135 kN、短期許容引張力は750 mm²×235 N/mm²。
    for (combined_axial_n, expected_ratio, expected_ok) in [
        (150_000.0, 40.0 / 47.0, true),
        (185_000.0, 148.0 / 141.0, false),
    ] {
        let forces = [combined_axial_n, 0.0, 0.0, 0.0, 0.0, 0.0];
        let report = run_member_design_checks(
            &model,
            &[(
                elem_id,
                MemberForces {
                    at: vec![(0.0, forces), (1.0, forces)],
                },
            )],
            &[],
            &MemberDesignCheckOptions {
                term: LoadTerm::Short,
                ..Default::default()
            },
        )
        .unwrap();

        assert!(!report.member_checks.is_empty());
        for (id, _, outcome) in report.member_checks {
            assert_eq!(id, elem_id);
            let CheckOutcome::Checked(result) = outcome else {
                panic!("{outcome:?}");
            };
            assert_eq!(result.components.len(), 1);
            assert_eq!(result.components[0].kind, CheckKind::Axial);
            assert!(
                (result.ratio() - expected_ratio).abs() < 1e-12,
                "{result:?}"
            );
            assert_eq!(result.ok(), expected_ok);
        }
    }
}
