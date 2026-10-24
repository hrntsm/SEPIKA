//! 並列モード（`sepika_math::parallelism`）の統合テスト。
//!
//! 並列度設定はプロセスグローバルのため、単一スレッド前提の決定性テスト
//! （unit テスト群）と干渉しないよう統合テスト（別プロセス）に分離している。
//! このファイル内のテストは同一プロセスで並行実行されるので、
//! すべて並列モードを前提とし、途中で `Deterministic` へ戻さない。

use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, NodeId, SectionId};
use sepika_core::model::{
    ElementData, ElementKind, EndCondition, ForceRegime, FrameSectionUse, LoadCase,
    LoadCombination, LocalAxis, Material, MaterialCategory, MemberLoad, MemberLoadKind, Model,
    NodalLoad, Node, Section,
};
use sepika_math::parallelism::{set_parallelism, Parallelism};
use sepika_solver::statics::analysis::Analysis;

/// 複数の荷重ケースを持つ片持ち梁モデル（unit テストの
/// `make_cantilever_model` と同等。先端に方向違いの集中荷重を n ケース）。
fn make_model(n_cases: usize) -> Model {
    let load_cases: Vec<LoadCase> = (0..n_cases)
        .map(|i| {
            let mut values = [0.0; 6];
            // ケースごとに方向・大きさを変える（x/y/z の集中荷重）
            values[i % 3] = 100.0 * (i as f64 + 1.0);
            LoadCase {
                kind: Default::default(),
                id: LoadCaseId(i as u32 + 1),
                name: format!("case{}", i + 1),
                nodal: vec![NodalLoad::manual(NodeId(1), values)],
                member: Vec::new(),
            }
        })
        .collect();
    Model {
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
                coord: [1000.0, 0.0, 0.0],
                restraint: Dof6Mask::FREE,
                mass: None,
                story: None,
                support_spring: None,
            },
        ],
        elements: vec![ElementData {
            id: ElemId(0),
            kind: ElementKind::Beam,
            nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 0.0, 1.0],
            },
            end_cond: [EndCondition::Fixed, EndCondition::Fixed],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            vertical_eccentricity: Some(0.0),
            plastic_zone: None,
            spring: None,
        }],
        sections: vec![Section {
            frame_use: Some(FrameSectionUse::Girder),
            id: SectionId(0),
            name: "beam".into(),
            area: 100.0,
            iy: 833.33,
            iz: 833.33,
            j: 100.0,
            depth: 10.0,
            width: 10.0,
            as_y: 83.33,
            as_z: 83.33,
            floor: None,
            panel_thickness: None,
            thickness: None,
            shape: None,
            material: Some(MaterialId(0)),
            rebar_material: None,
            shear_rebar_material: None,
            steel_material: None,
            property_basis: Default::default(),
        }],
        materials: vec![Material {
            strength_factor: None,
            concrete_class: Default::default(),
            id: MaterialId(0),
            name: "mat".into(),
            category: MaterialCategory::Steel,
            young: 20000.0,
            poisson: 0.3,
            density: 0.0,
            shear: None,
            fc: None,
            fy: None,
        }],
        load_cases,
        combinations: vec![LoadCombination {
            name: "combo1".into(),
            terms: vec![(LoadCaseId(1), 1.2), (LoadCaseId(2), 1.5)],
        }],
        ..Default::default()
    }
}

/// 並列バッチの結果が、同じ並列設定での個別呼び出しと一致すること
/// （ケース間に共有可変状態がなく、実行順が結果に影響しないことの検証）。
#[test]
fn test_parallel_batch_matches_individual() {
    set_parallelism(Parallelism::Threads(2));
    let model = make_model(8);
    let analysis = Analysis::prepare(&model).unwrap();

    let lcs: Vec<LoadCaseId> = (1..=8).map(LoadCaseId).collect();
    let batch = analysis.linear_static_batch(&lcs);
    assert_eq!(batch.len(), 8);
    for (i, lc) in lcs.iter().enumerate() {
        let individual = analysis.linear_static(*lc).unwrap();
        let b = batch[i].as_ref().unwrap();
        assert_eq!(b.disp, individual.disp, "case {} disp mismatch", i + 1);
        assert_eq!(
            b.member_forces.len(),
            individual.member_forces.len(),
            "case {} member_forces length mismatch",
            i + 1
        );
    }
}

/// 並列モードでも解析値が物理的に正しいこと（軸引張の理論解と比較）。
#[test]
fn test_parallel_solve_value_correct() {
    set_parallelism(Parallelism::Auto);
    let model = make_model(3);
    let analysis = Analysis::prepare(&model).unwrap();
    let res = analysis.linear_static(LoadCaseId(1)).unwrap();
    // case1 は x 方向 100 の軸引張: u = PL/EA
    let expected = 100.0 * 1000.0 / (20000.0 * 100.0);
    let ux = res.disp[1][0];
    assert!(
        (ux - expected).abs() < 1e-9,
        "ux={} expected={}",
        ux,
        expected
    );
}

/// 並列組合せバッチが逐次の組合せ解と（値として）一致すること。
#[test]
fn test_parallel_combination_batch() {
    set_parallelism(Parallelism::Threads(2));
    let model = make_model(4);
    let analysis = Analysis::prepare(&model).unwrap();
    let combos = vec![
        model.combinations[0].clone(),
        LoadCombination {
            name: "combo2".into(),
            terms: vec![(LoadCaseId(3), 1.0), (LoadCaseId(4), -0.5)],
        },
    ];
    let batch = analysis.linear_combination_batch(&combos);
    for (i, combo) in combos.iter().enumerate() {
        let individual = analysis.linear_combination(combo).unwrap();
        assert_eq!(
            batch[i].as_ref().unwrap().disp,
            individual.disp,
            "combo {} mismatch",
            i
        );
    }
}

mod static595_fixture {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test_fixtures/static595.rs"
    ));
}

#[test]
fn parallel_settings_use_case_local_active_sets_and_two_stiffness_classes() {
    use sepika_core::model::LoadCaseKind;
    set_parallelism(Parallelism::Threads(2));
    for kind in [
        ElementKind::Fiber,
        ElementKind::Brace { tension_only: true },
    ] {
        let mut model = static595_fixture::scalar_model(kind);
        model.stress_cfg.no_long_axial_column = true;
        model.stress_cfg.no_long_axial_brace = true;
        model.stress_cfg.tension_only_iteration = true;
        for (id, load_kind, force) in [
            (1, LoadCaseKind::Dead, 1000.0),
            (2, LoadCaseKind::Seismic, -1000.0),
        ] {
            let mut case = model.load_cases[0].clone();
            case.id = LoadCaseId(id);
            case.kind = load_kind;
            case.nodal[0].values[2] = force;
            model.load_cases.push(case);
        }
        let analysis = Analysis::prepare(&model).unwrap();
        let ids = [LoadCaseId(0), LoadCaseId(1), LoadCaseId(2), LoadCaseId(0)];
        let batch = analysis.linear_static_batch(&ids);
        for (id, result) in ids.iter().zip(batch) {
            let beta = if id.0 == 2 { 1.0 } else { 1e-6 };
            let factor = beta
                * if matches!(kind, ElementKind::Brace { .. }) && id.0 != 1 {
                    1e-6
                } else {
                    1.0
                };
            let force = if id.0 == 1 { 1000.0 } else { -1000.0 };
            let expected_u = force / (200000.0 + factor * 200000.0);
            let expected_n = factor * 200000.0 * expected_u;
            let result = result.unwrap();
            assert!((result.disp[1][2] / expected_u - 1.0).abs() < 1e-10);
            assert!((result.member_forces[0].1.at[0].1[0] / expected_n - 1.0).abs() < 1e-10);
            assert_eq!(result.disp, analysis.linear_static(*id).unwrap().disp);
        }
    }
}

#[test]
fn public_static_nonconvergence_is_case_local_and_combinations_propagate_failure() {
    use sepika_math::solver::SolveError;
    set_parallelism(Parallelism::Threads(2));
    let mut model = static595_fixture::cycling_brace_model();
    let mut zero = model.load_cases[0].clone();
    zero.id = LoadCaseId(1);
    zero.name = "zero".into();
    zero.nodal.clear();
    model.load_cases.push(zero);
    let analysis = Analysis::prepare(&model).unwrap();
    assert!(
        matches!(sepika_solver::statics::linear::linear_static_once(&model, LoadCaseId(0)), Err(SolveError::NonConvergence(message)) if message.contains("50") && message.contains("cycling-braces"))
    );
    let batch = analysis.linear_static_batch(&[LoadCaseId(0), LoadCaseId(1), LoadCaseId(0)]);
    assert!(matches!(&batch[0], Err(SolveError::NonConvergence(_))));
    assert!(batch[1].is_ok());
    assert!(matches!(&batch[2], Err(SolveError::NonConvergence(_))));
    let combo = LoadCombination {
        name: "failed dependency".into(),
        terms: vec![(LoadCaseId(0), 1.0), (LoadCaseId(1), 1.0)],
    };
    assert!(matches!(
        analysis.linear_combination(&combo),
        Err(SolveError::NonConvergence(_))
    ));
    let batch = analysis.linear_static_with_combinations(&[LoadCaseId(0), LoadCaseId(1)], &[combo]);
    assert!(batch.combos[0].is_err());
    assert!(batch.cases[1].is_ok());
}

#[test]
fn loaded_frame_and_legacy_brace_cases_remain_independent_in_parallel() {
    use sepika_core::model::LoadCaseKind;
    use sepika_solver::statics::linear::linear_static_once;
    set_parallelism(Parallelism::Threads(2));
    for frame_kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        for iterative in [false, true] {
            for fixed in [false, true] {
                let mut model = static595_fixture::scalar_model(frame_kind);
                let mut brace = model.elements[0].clone();
                brace.id = ElemId(1);
                brace.kind = ElementKind::Brace { tension_only: true };
                let mut section = model.sections[0].clone();
                section.id = SectionId(1);
                section.frame_use = Some(FrameSectionUse::Brace);
                brace.section = Some(section.id);
                model.elements.push(brace);
                model.sections.push(section);
                model.stress_cfg.tension_only_iteration = iterative;
                model.stress_cfg.no_long_axial_column = true;
                model.stress_cfg.no_long_axial_brace = true;
                if fixed {
                    model.nodes[1].restraint = Dof6Mask::FIXED;
                }
                model.load_cases.clear();
                for (id, kind, sign) in [
                    (0, LoadCaseKind::Dead, 1.0),
                    (1, LoadCaseKind::Seismic, -1.0),
                ] {
                    model.load_cases.push(LoadCase {
                        id: LoadCaseId(id),
                        name: format!("loaded-{id}"),
                        kind,
                        nodal: vec![NodalLoad::manual(
                            NodeId(1),
                            [0.0, 0.0, sign * 1000.0, 0.0, 0.0, 0.0],
                        )],
                        member: vec![
                            MemberLoad::manual(
                                ElemId(0),
                                [0.0, 0.0, sign],
                                MemberLoadKind::Point { a: 500.0, p: 600.0 },
                            ),
                            MemberLoad::manual(
                                ElemId(1),
                                [0.0, 0.0, sign],
                                MemberLoadKind::Point { a: 500.0, p: 200.0 },
                            ),
                        ],
                    });
                }
                let analysis = Analysis::prepare(&model).unwrap();
                let expected: Vec<_> = model
                    .load_cases
                    .iter()
                    .map(|case| linear_static_once(&model, case.id).unwrap())
                    .collect();
                if !fixed {
                    assert!((expected[0].disp[1][2] - 1400.0 / 200000.4).abs() < 1e-12);
                }
                for ids in [
                    [LoadCaseId(0), LoadCaseId(1), LoadCaseId(0)],
                    [LoadCaseId(1), LoadCaseId(0), LoadCaseId(1)],
                ] {
                    for (id, result) in ids.into_iter().zip(analysis.linear_static_batch(&ids)) {
                        let actual = result.unwrap();
                        let expected = &expected[id.index()];
                        assert_eq!(actual.disp, expected.disp);
                        assert_eq!(actual.panel_moments, expected.panel_moments);
                        assert_eq!(actual.member_forces.len(), expected.member_forces.len());
                        for (actual, expected) in
                            actual.member_forces.iter().zip(&expected.member_forces)
                        {
                            assert_eq!(actual.0, expected.0);
                            assert_eq!(actual.1.at, expected.1.at);
                        }
                    }
                }
            }
        }
    }
}
