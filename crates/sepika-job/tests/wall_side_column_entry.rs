#[path = "../../sepika-solver/tests/support/wall_side_column.rs"]
mod fixture;

#[test]
fn time_history_job_preserves_actual_wall_connection_reason_before_damping_eigen() {
    use sepika_core::ids::StoryId;
    use sepika_core::model::{AnalysisKind, ElementKind, Story, WallDlSupport};
    use sepika_element::behavior::MassOption;
    use sepika_element::factory::ensure_wall_side_column_analysis;
    use sepika_job::{compute::compute_time_history, AnalysisSettings};
    use sepika_solver::dynamic::timehistory::GroundMotion;
    for representation in [ElementKind::Beam, ElementKind::Fiber] {
        let mut model = fixture::wall_connection_fixture(representation);
        model.wall_plates[0].dl_support = Some(WallDlSupport::LowerBeam);
        model.stories = vec![
            Story::new(StoryId(0), "1F".into(), Some(0.0), Some(0.0)),
            Story::new(StoryId(1), "2F".into(), Some(3000.0), Some(0.0)),
        ];
        model.sections[0].rebar_material = None;
        let settings = AnalysisSettings {
            th_nonlinear: true,
            ..Default::default()
        };
        let expected = ensure_wall_side_column_analysis(
            &model,
            Some(AnalysisKind::TimeHistory),
            "非線形時刻歴解析",
            Some(MassOption::Consistent),
        )
        .unwrap_err();
        model
            .elements
            .retain(|element| element.kind != ElementKind::Wall);
        model.generated_wall_origins.clear();
        let report = sepika_job::prepare_model_for_analysis(&mut model, &settings, None)
            .unwrap()
            .frame_preparation
            .analysis(&model, &settings)
            .unwrap();
        let member = model.elements[0].id;
        let linear = report
            .decision(member, None)
            .unwrap()
            .as_ref()
            .err()
            .unwrap();
        assert!(linear.reason().contains("主筋の材料が未割当"));
        let nonlinear = report
            .decision(member, Some(AnalysisKind::TimeHistory))
            .unwrap()
            .as_ref()
            .err()
            .unwrap();
        assert_eq!(nonlinear.reason(), expected);
        let result = compute_time_history(
            model,
            settings,
            GroundMotion {
                dt: 0.01,
                accel_x: vec![0.0, 0.0],
                accel_y: None,
                accel_theta: None,
            },
        );
        let error = result.unwrap_err().to_string();
        assert!(error.contains(&expected), "{error}");
        assert!(!error.contains("固有値解析"));
    }
}
#[test]
fn linear_time_history_job_preserves_consistent_mass_reason_before_damping_eigen() {
    use sepika_core::model::{ElementKind, EndCondition};
    use sepika_element::behavior::MassOption;
    use sepika_element::factory::ensure_wall_side_column_analysis;
    use sepika_job::{compute::compute_time_history, AnalysisSettings};
    for kind in [ElementKind::Beam, ElementKind::Fiber] {
        let mut model = fixture::wall_connection_fixture(kind);
        for element in &mut model.elements[..2] {
            element.end_cond = [EndCondition::Fixed, EndCondition::Pinned];
        }
        let expected = ensure_wall_side_column_analysis(
            &model,
            None,
            "線形時刻歴解析",
            Some(MassOption::Consistent),
        )
        .unwrap_err();
        model.elements.retain(|e| e.kind != ElementKind::Wall);
        model.generated_wall_origins.clear();
        let error =
            compute_time_history(model, AnalysisSettings::default(), short_wave()).unwrap_err();
        assert!(matches!(error, sepika_job::JobError::InvalidInput(reason) if reason == expected));
    }
}

fn short_wave() -> sepika_solver::dynamic::timehistory::GroundMotion {
    sepika_solver::dynamic::timehistory::GroundMotion {
        dt: 0.01,
        accel_x: vec![0.0, 0.0, 0.0],
        accel_y: None,
        accel_theta: None,
    }
}

#[test]
fn pure_steel_walls_are_rejected_and_unloaded_frames_stay_at_rest() {
    use sepika_core::dof::Dof6Mask;
    use sepika_core::ids::MaterialId;
    use sepika_core::model::{ElementKind, MassMethod};
    use sepika_job::{compute::compute_time_history, AnalysisSettings};
    for kind in [ElementKind::Beam, ElementKind::Fiber] {
        for steel_wall in [false, true] {
            let mut model = fixture::wall_connection_fixture(kind);
            model.mass_method = MassMethod::LumpedOnly;
            model.materials[1].name = "SD390".into();
            model.materials[1].fy = Some(390.0);
            model.wall_plates[0].dl_support = Some(sepika_core::model::WallDlSupport::LowerBeam);
            for node in &mut model.nodes {
                node.restraint = if node.coord[2] == 0.0 {
                    Dof6Mask::FIXED
                } else {
                    Dof6Mask(0b111110)
                };
                node.mass = Some([1.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
            }
            if steel_wall {
                model.sections[2].shape = None;
                model.sections[2].property_basis.area = sepika_core::model::PropertyBasis::Supplied;
                model.sections[2].property_basis.j = sepika_core::model::PropertyBasis::Supplied;
                model.sections[2].material = Some(MaterialId(2));
                model.sections[2].thickness = Some(6.0);
            } else {
                model.wall_plates.clear();
                model.wall_regions.clear();
                model.wall_assignment_regions = Default::default();
            }
            model.elements.retain(|e| e.kind != ElementKind::Wall);
            model.generated_wall_origins.clear();
            if steel_wall {
                use sepika_core::ids::LoadCaseId;
                use sepika_job::compute;
                use sepika_job::JobError;
                let settings = AnalysisSettings::default();
                for error in [
                    compute::compute_linear_static(model.clone(), LoadCaseId(0)).unwrap_err(),
                    compute::compute_eigen(model.clone(), 1).unwrap_err(),
                    compute::compute_seismic(model.clone(), Default::default(), 0.25).unwrap_err(),
                    compute::compute_pushover(model.clone(), settings).unwrap_err(),
                    compute::compute_time_history(
                        model.clone(),
                        AnalysisSettings {
                            th_nonlinear: false,
                            ..settings
                        },
                        short_wave(),
                    )
                    .unwrap_err(),
                    compute::compute_time_history(
                        model.clone(),
                        AnalysisSettings {
                            th_nonlinear: true,
                            ..settings
                        },
                        short_wave(),
                    )
                    .unwrap_err(),
                    compute::compute_lumped_mass(model, settings, None, None, None, None, None)
                        .unwrap_err(),
                ] {
                    assert!(
                        matches!(&error, JobError::InvalidInput(reason)
                            if reason.contains("鋼板壁版 ID 0")
                                && reason.contains("重量は荷重として入力")),
                        "{error}"
                    );
                }
                continue;
            }
            for nonlinear in [false, true] {
                let mut input = model.clone();
                if !nonlinear {
                    use sepika_core::ids::{ElemId, LoadCaseId, NodeId};
                    use sepika_core::model::{
                        LoadCase, LoadCaseKind, MemberLoad, MemberLoadKind, NodalLoad,
                    };
                    input.load_cases.push(LoadCase {
                        origin: Default::default(),
                        id: LoadCaseId(0),
                        name: "線形時刻歴では載荷しない".into(),
                        kind: LoadCaseKind::Dead,
                        nodal: vec![NodalLoad::manual(
                            NodeId(2),
                            [700.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                        )],
                        member: vec![MemberLoad::manual(
                            ElemId(0),
                            [1.0, 0.0, 0.0],
                            MemberLoadKind::Distributed {
                                a: 0.0,
                                b: 3000.0,
                                w1: 2.0,
                                w2: 2.0,
                            },
                        )],
                    });
                }
                let result = compute_time_history(
                    input,
                    AnalysisSettings {
                        th_nonlinear: nonlinear,
                        th_apply_long_term: false,
                        th_damping: 0.0,
                        th_record_every: 1,
                        ..Default::default()
                    },
                    short_wave(),
                );
                let result =
                    result.unwrap_or_else(|e| panic!("{kind:?} nonlinear={nonlinear}: {e}"));
                assert_eq!(result.nonlinear, nonlinear);
                assert!(!result.applied_long_term);
                assert_eq!(result.non_converged_steps, 0);
                assert!(result.peak_disp.iter().flatten().all(|v| *v == 0.0));
                let recording = result.recording.unwrap();
                assert_eq!(recording.frame_time, vec![0.0, 0.01, 0.02]);
                assert!(recording
                    .member_forces
                    .iter()
                    .flatten()
                    .flatten()
                    .flat_map(|m| &m.at)
                    .flat_map(|(_, f)| f)
                    .all(|v| *v == 0.0));
            }
        }
    }
}
