use std::f64::consts::TAU;

use sepika_core::dof::{Dof, Dof6Mask, DofMap, DOF_PER_NODE};
use sepika_core::ids::NodeId;
use sepika_core::model::{Constraint, LoadCaseKind, Model, Node, EX_CASE_NAME, EY_CASE_NAME};
use sepika_core::units::GRAVITY_MM_S2;
use sepika_element::behavior::MassOption;
use sepika_job::compute::compute_linear_static;
use sepika_job::prepare::prepare_model_for_analysis;
use sepika_job::weight_preparation::weights_are_current;
use sepika_job::{build_lumped_mass, LumpedMassBuildInput};
use sepika_solver::common::assemble::assemble_global_m;
use sepika_solver::dynamic::lumped_mass::{
    lumped_mass_eigen, LumpedMassType, LumpedStiffnessSource, StickDim,
};
use sepika_solver::statics::analysis::SeismicDir;

#[allow(dead_code)]
#[path = "../../sepika-design-jp/tests/fixtures/eccentricity_model.rs"]
mod eccentricity_model;
#[path = "../../sepika-design-jp/tests/fixtures/multi_floor_torsional_cancellation_model.rs"]
mod multi_floor_model;
#[path = "../../sepika-design-jp/tests/fixtures/multi_floor_torsional_cancellation_reference.rs"]
mod multi_floor_reference;
use eccentricity_model::{column_model, TOP};
use multi_floor_model::{
    add_head_load_case, model_with_two_diaphragms, scale, HEADS, SMALL_SCALE, UNIFORM_MM, XY_MM,
};
use multi_floor_reference::uniform_displacement_reactions;

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "actual={actual}, expected={expected}"
    );
}

fn assert_column_hold(load: &str, reason: &str, drift_reason: &str) {
    assert!(
        (0..HEADS.len()).any(|i| reason.contains(&format!("階1・X方向の柱{i}:"))),
        "{load}: {reason}"
    );
    assert!(reason.contains(drift_reason), "{load}: {reason}");
    assert!(reason.contains("生成を保留"), "{load}: {reason}");
}

fn add_seismic_live_weight(model: &mut Model) {
    let weight = add_head_load_case(model, "比較用重量", [[0.0; 2]; 4]);
    let case = &mut model.load_cases[weight.index()];
    case.kind = LoadCaseKind::LiveSeismic;
    for load in &mut case.nodal {
        load.values[2] = -GRAVITY_MM_S2;
    }
}

fn prepare_comparison_mass(model: &mut Model) {
    let mut mass_model = column_model(&XY_MM, 0.0, 0.0);
    add_seismic_live_weight(&mut mass_model);
    prepare_model_for_analysis(&mut mass_model, &Default::default(), None).unwrap();
    model.stories[TOP.index()].dynamic_mass = mass_model.stories[TOP.index()].dynamic_mass;
}

#[test]
fn planar_column_ki_requires_compatible_column_drifts_under_proportional_loads() {
    let [uniform_x, uniform_y] = uniform_displacement_reactions();
    let uniform_k = uniform_x.iter().map(|load| load[0]).sum::<f64>() / UNIFORM_MM;
    let uniform_period = TAU * (4.0 / uniform_k).sqrt();
    let unit_couple = [[1.0, 0.0], [0.0; 2], [-1.0, 0.0], [0.0; 2]];

    let mut model = model_with_two_diaphragms();
    prepare_comparison_mass(&mut model);

    let unit = add_head_load_case(&mut model, "単位偶力", unit_couple);
    let unit_response = compute_linear_static(model.clone(), unit).unwrap();
    let c = HEADS[2].index();
    let couple_n = (SMALL_SCALE - 1.0) * UNIFORM_MM / unit_response.disp[c][0];
    let mut twisted_x = uniform_x;
    twisted_x[0][0] += couple_n;
    twisted_x[2][0] -= couple_n;

    let mut observe = |name, forces, expected_shears: [f64; 2], expected_c_drift| {
        let case = add_head_load_case(&mut model, name, forces);
        let response = compute_linear_static(model.clone(), case).unwrap();
        let shears = response.story_shears.as_ref().unwrap();
        close(shears[0][0], expected_shears[0], 1e-8);
        close(shears[1][0], expected_shears[1], 1e-8);
        close(response.disp[c][0], expected_c_drift, 1e-11);
        let result = build_lumped_mass(LumpedMassBuildInput {
            model_type: LumpedMassType::EquivalentShear,
            model: &model,
            dim: StickDim::Planar,
            source: LumpedStiffnessSource::ColumnKi,
            dir: SeismicDir::X,
            nonlinear: false,
            secant_ratio: 0.75,
            target_drift_angle: 1.0 / 200.0,
            res_x: Some(&response),
            res_y: None,
            po_x: None,
            po_y: None,
        });
        let shear_n = response.column_end_horizontal_forces[2].1.as_ref().unwrap()[1][0];
        let head_displacements_mm = HEADS.map(|head| response.disp[head.index()]);
        (result, shear_n, head_displacements_mm)
    };

    let full_shear = uniform_k * UNIFORM_MM;
    let mut twisted_column_shear = None;
    for (factor, control, twisted) in [
        (1.0, "X対照", "Xねじれ"),
        (SMALL_SCALE, "X対照縮小", "Xねじれ縮小"),
    ] {
        let expected_shears = [factor * full_shear, 0.0];
        let (result, control_shear_n, _) = observe(
            control,
            scale(uniform_x, factor),
            expected_shears,
            factor * UNIFORM_MM,
        );
        let result = result.unwrap_or_else(|err| panic!("{control}: {err}"));
        let [story] = result.stories.as_slice() else {
            panic!("{control}: 層ばねは1個必要です");
        };
        close(story.mass, 4.0, 1e-10);
        close(story.skeleton.k1, uniform_k, 1e-6);
        let modal = lumped_mass_eigen(&result, 1).unwrap();
        let [period_s] = modal.period.as_slice() else {
            panic!("{control}: 固有周期は1個必要です");
        };
        close(*period_s, uniform_period, 1e-10);
        close(control_shear_n, uniform_x[2][0] * factor, 1e-9);

        let (result, shear_n, _) = observe(
            twisted,
            scale(twisted_x, factor),
            expected_shears,
            factor * SMALL_SCALE * UNIFORM_MM,
        );
        assert!(shear_n.abs() > 20.0 * SMALL_SCALE * control_shear_n.abs());
        if let Some(full_column_shear) = twisted_column_shear {
            close(shear_n, factor * full_column_shear, 1e-9);
        } else {
            twisted_column_shear = Some(shear_n);
        }
        assert_column_hold(twisted, &result.unwrap_err().to_string(), "層間変位");
    }

    let combined = std::array::from_fn(|i| {
        std::array::from_fn(|axis| uniform_x[i][axis] + uniform_y[i][axis])
    });
    let y_shear = uniform_y.iter().map(|load| load[1]).sum::<f64>();
    let (result, _, head_displacements_mm) =
        observe("X・Y一様並進", combined, [full_shear, y_shear], UNIFORM_MM);
    for displacements_mm in head_displacements_mm {
        close(displacements_mm[0], UNIFORM_MM, 1e-11);
        close(displacements_mm[1], UNIFORM_MM, 1e-11);
    }
    assert_column_hold(
        "X・Y一様並進",
        &result.unwrap_err().to_string(),
        "直交方向に層間変位",
    );
}

#[test]
fn two_diaphragms_require_unique_ownership_and_explicit_user_weights() {
    let settings = sepika_job::AnalysisSettings::default();
    let mut shared_heads = model_with_two_diaphragms();
    for head in HEADS {
        shared_heads.nodes[head.index()].mass = None;
    }
    add_seismic_live_weight(&mut shared_heads);

    let mut dedicated = shared_heads.clone();
    let reps = [NodeId(8), NodeId(9)];
    let centers = [[0.0, 2_000.0], [2_000.0, 2_000.0]];
    let mut restraint = Dof6Mask::FREE;
    for dof in [Dof::Uz, Dof::Rx, Dof::Ry] {
        restraint.set_fixed(dof);
    }
    for (id, [x, y]) in reps.into_iter().zip(centers) {
        dedicated.nodes.push(Node {
            id,
            coord: [x, y, 3_000.0],
            restraint,
            mass: None,
            story: Some(TOP),
            support_spring: None,
        });
    }
    dedicated.constraints = [
        Constraint::rigid_diaphragm(TOP, reps[0], vec![HEADS[0], HEADS[2]]),
        Constraint::rigid_diaphragm(TOP, reps[1], vec![HEADS[1], HEADS[3]]),
    ]
    .to_vec();

    let mut missing = dedicated.clone();
    let mut overlapping = dedicated.clone();
    for (model, members) in [
        (&mut missing, vec![HEADS[1]]),
        (&mut overlapping, vec![HEADS[1], HEADS[2], HEADS[3]]),
    ] {
        model.constraints[1] = Constraint::rigid_diaphragm(TOP, reps[1], members);
    }
    let mut wrong_weight = dedicated.clone();
    let mut with_weights = dedicated.clone();
    for (model, weights) in [
        (&mut wrong_weight, [19_613.30, 39_226.60]),
        (&mut with_weights, [19_613.30; 2]),
    ] {
        for (constraint, input) in model.constraints.iter_mut().zip(weights) {
            let Constraint::RigidDiaphragm { weight, .. } = constraint else {
                unreachable!();
            };
            *weight = Some(input);
        }
    }
    let mut with_representative_masses = with_weights.clone();
    let rep_mass = [2.0, 2.0, 0.0, 0.0, 0.0, 8_000_000.0];
    for rep in reps {
        with_representative_masses.nodes[rep.index()].mass = Some(rep_mass);
    }
    let mut duplicate_mass = with_representative_masses.clone();
    for head in HEADS {
        duplicate_mass.nodes[head.index()].mass = Some([1.0, 1.0, 0.0, 0.0, 0.0, 0.0]);
    }

    let ownership_notice = "階 上階 の複数剛床へ重量・物理質量を一意に帰属できません";
    for (name, mut model, expected_notice, current, expected_rep_mass, nodal_mass_t) in [
        (
            "柱頭兼用",
            shared_heads,
            Some(ownership_notice),
            false,
            None,
            0.0,
        ),
        (
            "所属不足",
            missing,
            Some(ownership_notice),
            false,
            None,
            0.0,
        ),
        (
            "所属重複",
            overlapping,
            Some(ownership_notice),
            false,
            None,
            0.0,
        ),
        (
            "専用代表のみ",
            dedicated,
            Some("各剛床の有限の正の重量が必要"),
            true,
            None,
            0.0,
        ),
        (
            "過大剛床重量",
            wrong_weight,
            Some("剛床重量合計と採用した階重量が一致しません"),
            true,
            None,
            0.0,
        ),
        ("剛床重量設定", with_weights, None, true, None, 0.0),
        (
            "剛床重量・代表質量設定",
            with_representative_masses,
            None,
            true,
            Some(rep_mass),
            4.0,
        ),
        (
            "節点質量の二重入力",
            duplicate_mass,
            None,
            true,
            Some(rep_mass),
            8.0,
        ),
    ] {
        let input_floors: Vec<_> = model
            .diaphragms_of(TOP)
            .map(|floor| (floor.master, floor.slaves.to_vec(), floor.weight))
            .collect();
        let report = prepare_model_for_analysis(&mut model, &settings, None).unwrap();
        let prepared_floors: Vec<_> = model
            .diaphragms_of(TOP)
            .map(|floor| (floor.master, floor.slaves.to_vec(), floor.weight))
            .collect();
        assert_eq!(prepared_floors, input_floors, "{name}");
        if current {
            assert!(model.diaphragms_of(model.stories[0].id).count() == 1);
        }
        assert_eq!(
            weights_are_current(&model, settings.mass_method),
            current,
            "{name}"
        );
        for (axis, case_name) in [EX_CASE_NAME, EY_CASE_NAME].into_iter().enumerate() {
            let case = model.load_cases.iter().find(|case| case.name == case_name);
            if let Some(detail) = expected_notice {
                assert!(case.is_none(), "{name} {case_name}");
                assert!(
                    report.notices.iter().any(|notice| {
                        notice.starts_with(&format!("{case_name} の Ai 地震力"))
                            && notice.contains(detail)
                    }),
                    "{name}: {:?}",
                    report.notices
                );
            } else {
                assert!(report.notices.is_empty(), "{name}: {:?}", report.notices);
                let case = case.unwrap();
                assert_eq!(case.nodal.len(), 2, "{name} {case_name}");
                for rep in reps {
                    let load = case.nodal.iter().find(|load| load.node == rep).unwrap();
                    assert!(load.source.is_auto(), "{name} {case_name}");
                    for (i, value) in load.values.into_iter().enumerate() {
                        close(value, if i == axis { 3_922.66 } else { 0.0 }, 1e-8);
                    }
                }
            }
        }
        if current {
            let record = model.seismic_weight_generation.as_ref().unwrap();
            for (i, expected_n) in [0.0, 39_226.60].into_iter().enumerate() {
                let story = &model.stories[i];
                assert_eq!(record.calculated_weights[i].0, story.id, "{name}");
                close(record.calculated_weights[i].1, expected_n, 1e-8);
                close(story.seismic_weight.unwrap(), expected_n, 1e-8);
                close(
                    story.dynamic_mass.as_ref().unwrap().mass_equiv_weight_n,
                    expected_n,
                    1e-8,
                );
            }
            let top_mass = model.stories[TOP.index()].dynamic_mass.as_ref().unwrap();
            assert_eq!(top_mass.center_xy_mm, [1_000.0, 2_000.0], "{name}");
            close(top_mass.inertia_t_mm2, 20_000_000.0, 1e-7);
            for (rep, [x, y]) in reps.into_iter().zip(centers) {
                assert_eq!(model.nodes[rep.index()].coord, [x, y, 3_000.0], "{name}");
                assert_eq!(model.nodes[rep.index()].mass, expected_rep_mass, "{name}");
            }
            let dofmap = DofMap::build(&model);
            let mass = assemble_global_m(&model, &dofmap, MassOption::Lumped)
                .unwrap()
                .to_dense();
            for axis in 0..2 {
                let sum_t: f64 = HEADS
                    .into_iter()
                    .chain(reps)
                    .map(|node| {
                        let i = dofmap.active(node.index() * DOF_PER_NODE + axis).unwrap() as usize;
                        mass[(i, i)]
                    })
                    .sum();
                close(sum_t, nodal_mass_t, 1e-10);
            }
        } else {
            for story in &model.stories {
                assert!(story.seismic_weight.is_none(), "{name}");
                assert!(story.dynamic_mass.is_none(), "{name}");
            }
        }
        eprintln!("{name}: {:?}", report.notices);
    }
}
