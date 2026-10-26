use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId};
use sepika_core::model::{
    ElementData, ElementKind, EndCondition, ForceRegime, LocalAxis, Material, MaterialCategory,
    Model, Node, RigidZone,
};
use sepika_core::section_shape::{RcRectColumnRebar, RectColumnHoop, SectionShape};
use sepika_design_jp::rc::joint::JointShape;
use sepika_design_jp::secondary::src_rank::src_column_rank_ratios;
use sepika_design_jp::srrc::panel_zone::{src_panel_zone_check, SrcPanelInput};
use sepika_design_jp::ultimate::{collect_rc_ultimate_checks, UltimateShearOptions};

fn shape() -> SectionShape {
    SectionShape::SrcColumnRect {
        b: 600.0,
        d: 600.0,
        rebar: RcRectColumnRebar {
            main_dia: 25.0,
            x: vec![4],
            y: vec![2],
            cover: 40.0,
            hoop: RectColumnHoop {
                dia: 10.0,
                pitch: 100.0,
                legs_x: 2,
                legs_y: 2,
            },
        },
        steel_height: 400.0,
        steel_width: 200.0,
        steel_web_thick: 8.0,
        steel_flange_thick: 13.0,
    }
}

#[test]
fn source886_rank_ratios_are_reproduction_not_appendix_n0() {
    for (n, expected) in [
        (-2_000_000.0, 0.0),
        (0.0, 0.0),
        (2_000_000.0, 0.171_982_708_773_543_5),
        (11_629_076.052_252_904, 1.0),
    ] {
        let (n_ratio, m_ratio) =
            src_column_rank_ratios(&shape(), "SN400B", 24.0, 345.0, n).unwrap();
        assert!((n_ratio - expected).abs() < 1e-12);
        assert!((m_ratio - 0.479_761_357_753_609_9).abs() < 1e-12);
        println!("rank,n={n},n_ratio={n_ratio:.15},m_ratio={m_ratio:.15}");
    }
}

#[test]
fn source886_allowable_panel_fixed_capacities_and_boundary() {
    for (shape, ma) in [
        (JointShape::Cross, 483_516_000.0),
        (JointShape::Tee, 322_344_000.0),
        (JointShape::Knee, 322_344_000.0),
        (JointShape::Corner, 161_172_000.0),
    ] {
        for factor in [0.999_999, 1.0, 1.000_001] {
            let inp = SrcPanelInput {
                shape,
                fc: 24.0,
                concrete_class: Default::default(),
                long_term: false,
                col_width: 600.0,
                beam_width: 300.0,
                m_bd: 400.0,
                m_cd: 500.0,
                j_tw: 12.0,
                s_cd: 350.0,
                beam_is_steel: false,
                n_ratio: 15.0,
                h_ratio: 1.0,
                sum_beam_moments: ma * factor,
            };
            let result = src_panel_zone_check(&inp);
            assert!((result.ratio() - factor).abs() < 1e-12);
            println!(
                "allowable_panel,shape={shape:?},ma={ma},factor={factor},ratio={:.15}",
                result.ratio()
            );
        }
    }
}

#[test]
fn source886_src_is_excluded_from_rc_ultimate_collector() {
    let mut sec = shape().to_section(SectionId(0), "SRC独立断面".into());
    sec.material = Some(MaterialId(0));
    let mat = Material {
        id: MaterialId(0),
        name: "Fc24".into(),
        category: MaterialCategory::Concrete,
        young: 24_000.0,
        poisson: 0.2,
        density: 2.4e-9,
        shear: None,
        fc: Some(24.0),
        fy: None,
        strength_factor: None,
        concrete_class: Default::default(),
    };
    let nodes = [0.0, 3000.0]
        .into_iter()
        .enumerate()
        .map(|(i, z)| Node {
            id: NodeId(i as u32),
            coord: [0.0, 0.0, z],
            restraint: Dof6Mask::FREE,
            mass: None,
            story: None,
            support_spring: None,
        })
        .collect();
    let elem = ElementData {
        id: ElemId(0),
        kind: ElementKind::Beam,
        nodes: [NodeId(0), NodeId(1)].into_iter().collect(),
        section: Some(SectionId(0)),
        local_axis: LocalAxis {
            ref_vector: [1.0, 0.0, 0.0],
        },
        end_cond: [EndCondition::Fixed; 2],
        force_regime: ForceRegime::Auto,
        rigid_zone: RigidZone::default(),
        plastic_zone: None,
        spring: None,
        vertical_eccentricity: Some(0.0),
    };
    let model = Model {
        nodes,
        sections: vec![sec],
        materials: vec![mat],
        elements: vec![elem],
        ..Default::default()
    };
    let result = collect_rc_ultimate_checks(&model, &[], &UltimateShearOptions::default()).unwrap();
    assert!(result.is_empty());
    println!(
        "rc_ultimate_collector,src_members=1,results={}",
        result.len()
    );
}
