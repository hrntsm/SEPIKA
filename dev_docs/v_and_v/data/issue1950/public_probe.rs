include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/src_pushover_shear.rs"
));

const LENGTH_BENDING_MM: f64 = 3600.0;
const STEPS: usize = 100;

fn bending_beam(
    web_mm: f64,
    flange_mm: f64,
    manufacture: SrcHManufacture,
    regime: ForceRegime,
) -> Model {
    let mut model = restrained_member(FrameSectionUse::Girder, web_mm, flange_mm);
    model.elements[0].kind = ElementKind::Beam;
    model.elements[0].force_regime = regime;
    model.nodes[0].coord[2] = LENGTH_BENDING_MM;
    model.nodes[1].coord = [LENGTH_BENDING_MM, 0.0, LENGTH_BENDING_MM];
    model.elements[0].end_cond[1] = EndCondition::Pinned;
    model.stories[1].elevation = Some(LENGTH_BENDING_MM);
    model
        .constraints
        .push(sepika_core::model::Constraint::rigid_diaphragm(
            StoryId(1),
            NodeId(1),
            vec![],
        ));
    let Some(SectionShape::SrcBeamRect {
        steel_manufacture, ..
    }) = &mut model.sections[0].shape
    else {
        unreachable!();
    };
    *steel_manufacture = manufacture;
    model.materials[3].fy = None;
    model
}

fn bending_pushover(
    mut model: Model,
    limit_knm: f64,
    steps: usize,
) -> Result<PushoverResult, String> {
    model.nodes[1].restraint.set_free(Dof::Ux);
    model.nodes[1].restraint.set_free(Dof::Uy);
    let final_force_n = limit_knm * 1e6 / LENGTH_BENDING_MM;
    model.stories[1].seismic_weight = Some(final_force_n / 0.2);
    let map = DofMap::build(&model);
    let reducer = Reducer::build(&model, &map);
    pushover_analysis_recording(
        &model,
        &map,
        &reducer,
        SeismicDir::Y,
        steps,
        PushoverTarget::from_max_disp(0.0),
        PushoverControl::LoadOnly,
        false,
        Default::default(),
    )
}

fn bending_report(label: &str, model: &Model, result: PushoverResult) {
    use sepika_core::model::AnalysisKind;
    use sepika_element::behavior::{Ctx, LocalVec};
    use sepika_element::factory::{build_nonlinear_behavior, StrengthBasis};

    let history = result.confirmed_history.as_ref().unwrap();
    let events = result
        .hinges
        .iter()
        .filter(|event| event.elem == ELEMENT)
        .count();
    println!(
        "{label}: steps={} end={:?} bending_events={events} fibers={} member_histories={}",
        history.len(),
        result.termination,
        result.fiber_states.len(),
        result.member_history.len()
    );
    let mut behavior = build_nonlinear_behavior(
        &model.elements[0],
        model,
        StrengthBasis::MaterialStrength,
        AnalysisKind::Incremental,
        None,
    );
    let ctx = Ctx { model };
    println!(
        "{label} spring={:?} fibers={}",
        behavior.end_spring_rotations(),
        behavior.fiber_section_states().is_some()
    );
    let mut prev = [0.; 3];
    let points: Vec<_> = history
        .iter()
        .zip(&result.steps)
        .map(|(step, state)| {
            let member = step
                .members
                .iter()
                .find(|member| member.elem == ELEMENT)
                .unwrap();
            let [ux, uy, rz] = state.floor_disp[1];
            let du = LocalVec {
                data: smallvec::smallvec![
                    0.,
                    0.,
                    0.,
                    0.,
                    0.,
                    0.,
                    ux - prev[0],
                    uy - prev[1],
                    0.,
                    0.,
                    0.,
                    rz - prev[2]
                ],
            };
            prev = [ux, uy, rz];
            let rotation = match behavior.update_state(&du, true, &ctx) {
                Ok(()) => {
                    let forces = behavior.internal_force(&ctx).data;
                    assert!((forces[5].abs() - member.m_strong).abs() < 100.0);
                    assert!(forces[0].abs() < 1.0 && forces[11].abs() < 100.0);
                    behavior.end_spring_rotations().map(|r| r[0].abs())
                }
                Err(reason) => panic!("step {}: {reason}", step.step),
            };
            assert!((member.m_strong - member.shear_strong * LENGTH_BENDING_MM).abs() < 10.0);
            assert!(member.axial.abs() < 1.0 && member.m_weak.abs() < 10.0);
            (step.step, uy, member.m_strong / 1e6, rotation)
        })
        .collect();
    let slopes: Vec<_> = points
        .windows(2)
        .map(|pair| (pair[1].2 - pair[0].2) / (pair[1].1 - pair[0].1))
        .collect();
    for (k, slope) in slopes.iter().enumerate() {
        if k == 0 || (slope / slopes[k - 1] - 1.0).abs() > 0.05 {
            println!(
                "segment {label} #{k}: {:?} -> {:?}, slope={slope:.8}",
                points[k],
                points[k + 1]
            );
        }
    }
    println!("last {label}: {:?}", points.last().unwrap());
}
fn bending_time_history(mut model: Model, accel_mm_s2: f64) -> Result<ResponseResult, SolveError> {
    model.nodes[1].restraint.set_free(Dof::Ux);
    model.nodes[1].restraint.set_free(Dof::Uy);
    model.nodes[1].mass = Some([1000.0, 1000.0, 1000.0, 0.0, 0.0, 0.0]);
    model.materials[0].density = 2.4e-9;
    model.materials[3].density = 7.85e-9;
    let map = DofMap::build(&model);
    let reducer = Reducer::build(&model, &map);
    nonlinear_time_history_analysis(
        &model,
        &map,
        &reducer,
        &GroundMotion {
            dt: 0.01,
            accel_x: vec![0.0; 301],
            accel_y: Some(
                (0..301)
                    .map(|i| accel_mm_s2 * f64::from(i) / 300.0)
                    .collect(),
            ),
            accel_theta: None,
        },
        &NewmarkCfg::average_accel(),
        &Damping::MassProportional { h: 0.0, omega: 1.0 },
        DampingAccumulation::default(),
        &[],
        &[],
        NonlinearThCfg {
            record_every: Some(1),
            ..NonlinearThCfg::default()
        },
    )
}

#[test]
fn issue1950_src_spring_public_probe() {
    use ForceRegime::{Auto, AxialBendingInteract, UniaxialBendingShear};
    use SrcHManufacture::{BuiltUp, Rolled, Unknown};

    let base = || bending_beam(45.0, 13.0, BuiltUp, Auto);
    for (label, model) in [
        ("auto", base()),
        (
            "explicit",
            bending_beam(45.0, 13.0, BuiltUp, UniaxialBendingShear),
        ),
        ("unknown", bending_beam(45.0, 13.0, Unknown, Auto)),
        ("rolled", bending_beam(45.0, 13.0, Rolled, Auto)),
        ("flange45", bending_beam(10.0, 45.0, BuiltUp, Auto)),
        ("web40", bending_beam(40.0, 13.0, BuiltUp, Auto)),
        ("steel_sn490b", {
            let mut model = base();
            model.materials[3].name = "SN490B".into();
            model
        }),
        ("steel_fy80", {
            let mut model = base();
            model.materials[3].fy = Some(80.0);
            model
        }),
        ("main_rebar_sd390", {
            let mut model = base();
            model.materials[1].name = "SD390".into();
            model.materials[1].fy = Some(390.0);
            model
        }),
        ("main_fy120", {
            let mut model = base();
            model.materials[0].fy = Some(120.0);
            model
        }),
    ] {
        bending_report(
            label,
            &model,
            bending_pushover(
                model.clone(),
                match label {
                    "flange45" => 8_500.0,
                    "main_fy120" => 3_900.0,
                    _ => 7_700.0,
                },
                STEPS,
            )
            .unwrap(),
        );
    }

    for manufacture in [BuiltUp, Unknown, Rolled] {
        let model = bending_beam(45.0, 13.0, manufacture, AxialBendingInteract);
        match bending_pushover(model.clone(), 100.0, 4) {
            Ok(result) => {
                assert_eq!(manufacture, BuiltUp);
                bending_report(&format!("interaction_{manufacture:?}"), &model, result);
            }
            Err(reason) => {
                let expected = match manufacture {
                    Unknown => "製造区分が不明",
                    Rolled => "圧延H形鋼のウェブ・フランジ",
                    BuiltUp => panic!("{reason}"),
                };
                assert!(reason.contains(expected));
                println!("interaction_{manufacture:?}: {reason}");
            }
        }
    }

    for (label, model) in [
        ("auto", base()),
        (
            "explicit",
            bending_beam(45.0, 13.0, BuiltUp, UniaxialBendingShear),
        ),
        ("unknown", bending_beam(45.0, 13.0, Unknown, Auto)),
        (
            "interaction_built",
            bending_beam(45.0, 13.0, BuiltUp, AxialBendingInteract),
        ),
        (
            "interaction_unknown",
            bending_beam(45.0, 13.0, Unknown, AxialBendingInteract),
        ),
    ] {
        let mut model = model;
        model.elements[0].end_cond = [EndCondition::Fixed; 2];
        match bending_time_history(
            model,
            if label.starts_with("interaction") {
                10.0
            } else {
                4_250.0
            },
        ) {
            Ok(result) => {
                assert!(result.nonlinear);
                assert_eq!(result.non_converged_steps, 0);
                assert_ne!(label, "interaction_unknown");
                let recording = result.recording.unwrap();
                let points: Vec<_> = recording
                    .frame_time
                    .iter()
                    .zip(&recording.node_disp)
                    .zip(&recording.member_forces)
                    .map(|((t, disp), forces)| {
                        let f = &forces[0].as_ref().unwrap().at;
                        assert_eq!([f[0].0, f[1].0], [0.0, 1.0]);
                        (*t, disp[1][1], f[0].1[5] / 1e6, f[1].1[5] / 1e6)
                    })
                    .collect();
                let slopes: Vec<_> = points
                    .windows(2)
                    .map(|pair| ((pair[1].2 - pair[0].2) / (pair[1].1 - pair[0].1)).abs())
                    .collect();
                for (k, slope) in slopes.iter().enumerate() {
                    if k == 0 || (slope / slopes[k - 1] - 1.0).abs() > 0.05 {
                        println!(
                            "th segment {label} #{k}: {:?} -> {:?}, slope={slope:.8}",
                            points[k],
                            points[k + 1]
                        );
                    }
                }
                println!("th last {label}: {:?}", points.last().unwrap());
            }
            Err(reason) => {
                assert_eq!(label, "interaction_unknown");
                assert!(reason.to_string().contains("製造区分が不明"));
                println!("th {label}: {reason}");
            }
        }
    }

    let err = bending_time_history(base(), 10.0).unwrap_err().to_string();
    assert!(err.contains("質量行列組立") && err.contains("Kbb が特異"));
    let mut keep = base();
    keep.girder_torsion = sepika_core::model::GirderTorsionMode::Keep;
    let result = bending_time_history(keep, 10.0).unwrap();
    assert!(result.nonlinear && result.history.node_disp.last().unwrap().abs() > 0.0);
    println!("th j端ピン: {err}");
    println!(
        "th j端ピン・ねじり保持: 最終Uy={:.9} mm",
        result.history.node_disp.last().unwrap()
    );
}
