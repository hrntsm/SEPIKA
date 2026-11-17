use sepika_core::adjacency::NodeAdjacency;
use sepika_core::dof::{Dof, Dof6Mask};
use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, NodeId, SectionId};
use sepika_core::model::*;
use sepika_design_jp::steel::buckling::steel_column_k_axes_with_index;
use sepika_design_jp::{
    run_member_design_checks, CheckKind, CheckOutcome, LoadTerm, MemberDesignCheckOptions,
};

fn portal(beam_i_ratio: f64) -> Model {
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

#[test]
fn ideal_pin_portal_records_automatic_k_below_independent_sway_values() {
    // 独立な柱の境界釣合い x tan(x)=6/G（0<x<π/2）の根。実装のG=10式から生成しない。
    for (beam_i_ratio, independent_k, observed_k) in [
        (10.0, 2.033325968414278, 1.695355703667181),
        (1.0, 2.327876759160841, 1.902969040512992),
        (0.1, 4.455745840042094, 3.010392659171542),
        (0.01, 12.953619106571256, 4.174291336465101),
    ] {
        let model = portal(beam_i_ratio);
        let index = NodeAdjacency::build(&model);
        for column in &model.elements[..2] {
            let (k, _) = steel_column_k_axes_with_index(&model, &index, column).unwrap();
            assert!((k - observed_k).abs() < 1e-12, "{k}");
            assert!(k < independent_k);
        }
    }
}

#[test]
fn ideal_pin_portal_public_analysis_and_checks_record_direct_length_verdict_change() {
    let mut model = portal(1.0);
    let solved = sepika_solver::statics::linear::linear_static_once(&model, LoadCaseId(0)).unwrap();
    for (id, forces) in &solved.member_forces {
        if id.0 < 2 {
            for (_, f) in &forces.at {
                assert!((f[0] + 2215200.0).abs() < 1e-7);
                assert!(f[1..].iter().all(|v| v.abs() < 1e-7));
            }
        }
    }
    // 同じ採用圧縮式へ独立Kを代入した期待値。圧縮式自体の規準適合は検証対象外。
    for (direct, expected_ratio) in [(false, 0.977460230503001), (true, 1.014743328291276)] {
        for attr in &mut model.steel_design_attrs {
            attr.lk_y_direct = direct.then_some(6983.630277482522);
        }
        let report = run_member_design_checks(
            &model,
            &solved.member_forces,
            &solved.panel_moments,
            &MemberDesignCheckOptions {
                term: LoadTerm::Long,
                ..Default::default()
            },
        )
        .unwrap();
        let mut count = 0;
        for (id, _, outcome) in report.member_checks {
            if id.0 >= 2 {
                continue;
            }
            count += 1;
            let CheckOutcome::Checked(result) = outcome else {
                panic!("{outcome:?}");
            };
            let axial = result
                .components
                .iter()
                .find(|c| c.kind == CheckKind::AxialBending)
                .unwrap();
            assert!((axial.ratio - expected_ratio).abs() < 1e-12, "{axial:?}");
            assert_eq!(result.ok(), !direct);
            println!(
                "direct={direct}, column={}, ratio={}, {}",
                id.0, axial.ratio, axial.detail
            );
        }
        assert_eq!(count, 6);
    }
}
