use sepika_core::dof::{Dof6Mask, DofMap};
use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId, StoryId};
use sepika_core::model::{
    ElementData, ElementKind, EndCondition, ForceRegime, LocalAxis, Material, MaterialCategory,
    Model, NodalLoad, Node, RigidZone, Story,
};
use sepika_core::section_shape::SectionShape;
use sepika_solver::common::constraint::Reducer;
use sepika_solver::nonlinear::pushover::*;
use sepika_solver::statics::analysis::SeismicDir;
fn base_story(node_ids: Vec<NodeId>) -> Story {
    Story {
        wall_weights: Vec::new(),
        level_kind: Default::default(),
        structure: Default::default(),
        id: StoryId(0),
        name: "1F".to_string(),
        elevation: Some(0.0),
        sl_offset_mm: Some(0.0),
        source_story: None,
        node_ids,
        seismic_weight: None,
        weight_override: None,
        dynamic_mass: None,
        standard_floor_load: None,
        column_finish_area_weight: 0.0,
        fireproof: Default::default(),
    }
}

fn column(
    rigid_zone: RigidZone,
    length_mm: f64,
) -> (
    Model,
    sepika_core::section_shape::RcRectColumnRebar,
    f64,
    f64,
) {
    use sepika_core::section_shape::{RcRectColumnRebar, RectColumnHoop};
    let rebar = RcRectColumnRebar {
        main_dia: 25.0,
        x: vec![5],
        y: vec![3],
        cover: 40.0,
        hoop: RectColumnHoop {
            dia: 10.0,
            pitch: 100.0,
            legs_x: 2,
            legs_y: 2,
        },
    };
    let (b, d) = (400.0, 600.0);
    let shape = SectionShape::RcColumnRect {
        b,
        d,
        rebar: rebar.clone(),
    };
    let mut sec = shape.to_section(SectionId(0), "RC-400x600".into());
    sec.frame_use = Some(sepika_core::model::FrameSectionUse::Column);
    sec.material = Some(MaterialId(0));
    sec.rebar_material = Some(MaterialId(1));
    sec.shear_rebar_material = Some(MaterialId(2));
    let mat = Material {
        strength_factor: None,
        concrete_class: Default::default(),
        id: MaterialId(0),
        name: "rc".to_string(),
        category: MaterialCategory::Concrete,
        young: 23000.0,
        poisson: 0.2,
        density: 0.0,
        shear: None,
        fc: Some(24.0),
        fy: None,
    };
    let rebar_mat = Material {
        id: MaterialId(1),
        name: "SD345".to_string(),
        category: MaterialCategory::Rebar,
        young: 205000.0,
        poisson: 0.3,
        fc: None,
        fy: Some(345.0),
        ..mat.clone()
    };
    let shear_mat = Material {
        id: MaterialId(2),
        name: "SD295A".to_string(),
        fy: Some(295.0),
        ..rebar_mat.clone()
    };
    let model = Model {
        nodes: vec![
            Node {
                id: NodeId(0),
                coord: [0.0, 0.0, 0.0],
                restraint: Dof6Mask::FIXED,
                mass: None,
                story: None,
                support_spring: None,
            },
            Node {
                id: NodeId(1),
                coord: [0.0, 0.0, length_mm],
                restraint: Dof6Mask::FREE,
                mass: None,
                story: Some(StoryId(1)),
                support_spring: None,
            },
        ],
        elements: vec![ElementData {
            id: ElemId(0),
            kind: ElementKind::Fiber,
            nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [1.0, 0.0, 0.0],
            },
            end_cond: [EndCondition::Fixed, EndCondition::Fixed],
            force_regime: ForceRegime::Auto,
            rigid_zone,
            vertical_eccentricity: Some(0.0),
            plastic_zone: None,
            spring: None,
        }],
        sections: vec![sec],
        materials: vec![mat, rebar_mat, shear_mat],
        ..Default::default()
    };
    (model, rebar, b, d)
}

#[test]
fn tension_shear_public_solver() {
    for (length_mm, fixed_rotation) in [(500.0, false), (600.0, false), (1200.0, true)] {
        for (weight, final_q) in [(3_300_000.0, 660_000.0), (4_000_000.0, 800_000.0)] {
            for axial in [-240_000.0, 0.0, 240_000.0] {
                let (mut model, _, _, _) = column(Default::default(), length_mm);
                if fixed_rotation {
                    model.nodes[1].restraint = Dof6Mask(0b111000);
                }
                model.load_cases = sepika_core::model::default_load_cases();
                model.combinations = sepika_core::model::default_combinations();
                model.ultimate_gravity_combination = Some(0);
                model.load_cases[0].nodal = vec![NodalLoad::manual(
                    NodeId(1),
                    [0.0, 0.0, -axial, 0.0, 0.0, 0.0],
                )];
                let mut upper = base_story(vec![NodeId(1)]);
                upper.id = StoryId(1);
                upper.elevation = Some(length_mm);
                upper.seismic_weight = Some(weight);
                model.stories = vec![base_story(vec![]), upper];
                let dofmap = DofMap::build(&model);
                let reducer = Reducer::build(&model, &dofmap);
                let result = pushover_analysis_recording(
                    &model,
                    &dofmap,
                    &reducer,
                    SeismicDir::X,
                    100,
                    PushoverTarget::from_max_disp(0.0),
                    PushoverControl::LoadOnly,
                    true,
                    false,
                    DuctilityMethod::default(),
                );
                match result {
                    Err(e) => {
                        println!("AXIAL={axial} ERROR={e}");
                        panic!("{e}");
                    }
                    Ok(r) => {
                        println!("LENGTH={length_mm} FIXED_ROT={fixed_rotation} WEIGHT={weight} AXIAL={axial} TERMINATION={:?} STEPS={} EVENTS={:?} RESPONSE={:?}",r.termination,r.steps.len(),r.shear_yields,r.member_response);
                        assert_eq!(
                            r.termination,
                            PushoverTermination::LambdaCap { lambda: 1.0 }
                        );
                        assert_eq!(r.steps.len(), 101);
                        let expected_first = if fixed_rotation {
                            Some(if final_q < 700_000.0 {
                                if axial > 0.0 {
                                    97
                                } else {
                                    94
                                }
                            } else if axial > 0.0 {
                                80
                            } else {
                                78
                            })
                        } else if final_q < 666_662.184_386_159_4 {
                            None
                        } else if axial > 0.0 {
                            Some(86)
                        } else {
                            Some(84)
                        };
                        assert_eq!(r.shear_yields.first().map(|e| e.step), expected_first);
                        let expected_events = expected_first
                            .map(|first| (first..=100).collect::<Vec<_>>())
                            .unwrap_or_default();
                        assert_eq!(
                            r.shear_yields.iter().map(|e| e.step).collect::<Vec<_>>(),
                            expected_events
                        );
                        assert!(r.shear_yields.iter().all(|e| e.elem == ElemId(0)));
                        assert!((r.steps.last().unwrap().base_shear - final_q).abs() < 0.1);
                        assert_eq!(r.member_history.len(), 1);
                        assert_eq!(r.member_history[0].records.len(), 101);
                        let mut max_q_residual: f64 = 0.0;
                        let mut max_n_residual: f64 = 0.0;
                        let mut max_m_residual: f64 = 0.0;
                        for (i, step) in r.steps.iter().enumerate() {
                            let h = r.member_history.first().and_then(|h| h.records.get(i));
                            let member = h.unwrap();
                            let q_residual = (step.base_shear - final_q * step.load_factor).abs();
                            let n_residual = (f64::from(member.n) - axial).abs();
                            let moment_arm = length_mm / if fixed_rotation { 2.0 } else { 1.0 };
                            let expected_tip = if fixed_rotation {
                                final_q * step.load_factor * moment_arm
                            } else {
                                0.0
                            };
                            let m_residual = (f64::from(member.mz_i).abs()
                                - final_q * step.load_factor * moment_arm)
                                .abs()
                                .max((f64::from(member.mz_j).abs() - expected_tip).abs());
                            assert!(q_residual < 1.0, "Q residual={q_residual}");
                            assert!(n_residual < 1.0, "N residual={n_residual}");
                            assert!(m_residual < 1024.0, "M residual={m_residual}");
                            max_q_residual = max_q_residual.max(q_residual);
                            max_n_residual = max_n_residual.max(n_residual);
                            max_m_residual = max_m_residual.max(m_residual);
                            if i <= 1 || (77..=98).contains(&i) || i == 100 {
                                println!(
                                    "STEP {i} lambda={} Q={} u={} history={h:?}",
                                    step.load_factor, step.base_shear, step.top_disp
                                );
                            }
                        }
                        println!("MAX_RESIDUAL Q_N={max_q_residual} N_N={max_n_residual} M_Nmm={max_m_residual}");
                    }
                }
            }
        }
    }
}
