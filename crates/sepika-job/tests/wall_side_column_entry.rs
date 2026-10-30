#[path = "../../sepika-solver/tests/support/wall_side_column.rs"]
mod fixture;

#[test]
fn time_history_job_preserves_actual_wall_connection_reason_before_damping_eigen() {
    use sepika_core::model::{AnalysisKind, ElementKind};
    use sepika_element::behavior::MassOption;
    use sepika_element::factory::ensure_wall_side_column_analysis;
    use sepika_job::{compute::compute_time_history, AnalysisSettings};
    use sepika_solver::dynamic::timehistory::GroundMotion;
    for representation in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        let mut model = fixture::wall_connection_fixture(representation);
        let expected = ensure_wall_side_column_analysis(
            &model,
            Some(AnalysisKind::TimeHistory),
            "非線形時刻歴解析",
            false,
            Some(MassOption::Consistent),
        )
        .unwrap_err();
        model
            .elements
            .retain(|element| element.kind != ElementKind::Wall);
        model.generated_wall_origins.clear();
        let result = compute_time_history(
            model,
            AnalysisSettings {
                th_nonlinear: true,
                ..Default::default()
            },
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
    for kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        let mut model = fixture::wall_connection_fixture(kind);
        for element in &mut model.elements[..2] {
            element.end_cond = [EndCondition::Fixed, EndCondition::Pinned];
        }
        let expected = ensure_wall_side_column_analysis(
            &model,
            None,
            "線形時刻歴解析",
            false,
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
fn ordinary_and_pure_steel_time_history_jobs_remain_valid_with_existing_notice() {
    use sepika_core::dof::Dof6Mask;
    use sepika_core::ids::{MaterialId, SectionId};
    use sepika_core::model::{ElementKind, MassMethod};
    use sepika_core::section_shape::SectionShape;
    use sepika_job::{compute::compute_time_history, AnalysisSettings};
    for kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
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
                for i in 0..2 {
                    let mut section = SectionShape::SteelBox {
                        height: 600.0,
                        width: 600.0,
                        thick: 12.0,
                        horizontal_thick: 12.0,
                        corner_r: Some(0.0),
                    }
                    .to_section(SectionId(i as u32), "鋼材".into());
                    section.frame_use = model.sections[i].frame_use;
                    section.material = Some(MaterialId(2));
                    model.sections[i] = section;
                }
                model.sections[2].shape = None;
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
            let (expanded, index, _) = sepika_core::wall_expand::expand_wall_elements(&model);
            let notice = sepika_job::notices::steel_seismic_wall_notice(&expanded, &index);
            assert_eq!(notice.is_some(), steel_wall);
            if let Some(notice) = notice {
                assert!(notice.contains("せん断座屈") && notice.contains("壁版 ID: 0"));
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
                )
                .unwrap_or_else(|e| {
                    panic!("{kind:?} steel_wall={steel_wall} nonlinear={nonlinear}: {e}")
                });
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
