use sepika_core::dof::{Dof6Mask, DofMap};
use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId};
use sepika_core::model::*;
use sepika_core::section_shape::SectionShape;
use sepika_element::behavior::MassOption;
use sepika_element::frame::beam::BeamElement;
use sepika_math::solver::{make_solver, SolveError, SolverBackend};
use sepika_solver::common::assemble::{assemble_global_k, assemble_global_m};
use sepika_solver::common::constraint::Reducer;
use sepika_solver::dynamic::damping::Damping;
use sepika_solver::dynamic::eigen::{solve_eigen, solve_eigen_with_solver};
use sepika_solver::dynamic::timehistory::{
    linear_time_history_analysis, linear_time_history_from_state, linear_time_history_with_state,
    GroundMotion, NewmarkCfg, TimeStepState,
};

fn cantilever(kind: ElementKind) -> Model {
    let mut section = SectionShape::SteelFlatBar {
        width: 200.0,
        thick: 20.0,
    }
    .to_section(SectionId(0), "C1".into());
    section.material = Some(MaterialId(0));
    section.frame_use = Some(FrameSectionUse::Column);
    Model {
        nodes: (0..2)
            .map(|id| Node {
                id: NodeId(id),
                coord: [0.0, 0.0, 3000.0 * f64::from(id)],
                restraint: if id == 0 {
                    Dof6Mask::FIXED
                } else {
                    Dof6Mask(0b111110)
                },
                mass: (id == 1).then_some([1.0; 6]),
                story: None,
                support_spring: None,
            })
            .collect(),
        elements: vec![ElementData {
            id: ElemId(0),
            kind,
            nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [1.0, 0.0, 0.0],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            plastic_zone: None,
            spring: None,
        }],
        sections: vec![section],
        materials: vec![Material {
            id: MaterialId(0),
            name: "SN400B".into(),
            category: MaterialCategory::Steel,
            young: 205000.0,
            poisson: 0.3,
            density: 0.0,
            shear: Some(80000.0),
            fc: None,
            fy: Some(235.0),
            strength_factor: None,
            concrete_class: Default::default(),
        }],
        mass_method: MassMethod::LumpedOnly,
        ..Default::default()
    }
}

#[test]
fn public_dynamic_entries_reject_invalid_beam_g_before_initial_resume_and_cached_solver_builds() {
    for kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        let baseline = cantilever(kind);
        let dofmap = DofMap::build(&baseline);
        let reducer = Reducer::build(&baseline, &dofmap);
        assert_eq!(reducer.n_indep, 1);
        let mut solver = make_solver(SolverBackend::DirectSparseCholesky);
        solver
            .factorize(&reducer.reduce_k(&assemble_global_k(&baseline, &dofmap)))
            .unwrap();
        let wave = GroundMotion {
            dt: 0.01,
            accel_x: vec![0.0; 3],
            accel_y: None,
            accel_theta: None,
        };
        let newmark = NewmarkCfg::average_accel();
        let damping = Damping::MassProportional { h: 0.0, omega: 1.0 };
        let state = TimeStepState {
            step: 0,
            time: 0.0,
            disp_red: vec![0.0],
            vel_red: vec![0.0],
            accel_red: vec![0.0],
        };
        for (shear, poisson, valid) in [
            (Some(80000.0), -1.0, true),
            (None, 0.3, true),
            (Some(f64::NAN), 0.3, false),
            (Some(f64::INFINITY), 0.3, false),
            (Some(f64::NEG_INFINITY), 0.3, false),
            (Some(0.0), 0.3, false),
            (Some(-1.0), 0.3, false),
            (None, -1.0, false),
            (None, -2.0, false),
            (None, f64::NAN, false),
            (None, f64::INFINITY, false),
        ] {
            let mut model = baseline.clone();
            model.materials[0].shear = shear;
            model.materials[0].poisson = poisson;
            if valid {
                solver
                    .factorize(&reducer.reduce_k(&assemble_global_k(&model, &dofmap)))
                    .unwrap();
            }
            let expected = BeamElement::try_new(&model.elements[0], &model).err();
            for result in [
                solve_eigen(&model, &dofmap, &reducer, 1).map(|_| ()),
                solve_eigen_with_solver(&model, &dofmap, &reducer, 1, solver.as_ref()).map(|_| ()),
                linear_time_history_analysis(
                    &model,
                    &dofmap,
                    &reducer,
                    &wave,
                    &newmark,
                    &damping,
                    &[],
                    &[],
                    false,
                    None,
                )
                .map(|_| ()),
                linear_time_history_with_state(
                    &model,
                    &dofmap,
                    &reducer,
                    &wave,
                    &newmark,
                    &damping,
                    &[],
                    &[],
                    false,
                    None,
                )
                .map(|_| ()),
                linear_time_history_from_state(
                    &model, &dofmap, &reducer, &wave, &newmark, &damping, &state, false, None,
                )
                .map(|_| ()),
                assemble_global_m(&model, &dofmap, MassOption::Consistent).map(|_| ()),
            ] {
                if valid {
                    result.unwrap();
                } else {
                    let SolveError::InvalidInput(reason) = result.unwrap_err() else {
                        panic!("不正Gは入力不備として拒否する");
                    };
                    assert!(reason.contains(expected.as_ref().unwrap()), "{reason}");
                    assert!(reason.contains("部材 ID 0") && reason.contains("せん断弾性係数 G"));
                }
            }
        }
    }
}
