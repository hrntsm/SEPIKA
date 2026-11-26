include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/cft_fiber_yield.rs"
));

#[derive(Clone, Copy, PartialEq)]
struct Sample {
    index: usize,
    at: f64,
    ux_mm: f64,
    mz_knm: f64,
    spring_rad: Option<f64>,
}

impl std::fmt::Display for Sample {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "#{} at={:.6} Ux={:.6} Mz={:.6} spring={:.8?}",
            self.index, self.at, self.ux_mm, self.mz_knm, self.spring_rad
        )
    }
}

fn issue1990_model(
    circular: bool,
    thickness_mm: f64,
    steel_fy: f64,
    concrete_fy: Option<f64>,
) -> Model {
    let shape = if circular {
        SectionShape::CftPipe {
            outer_dia: 400.0,
            thick: thickness_mm,
        }
    } else {
        SectionShape::CftBox {
            height: 400.0,
            width: 400.0,
            thick: thickness_mm,
            horizontal_thick: thickness_mm,
            corner_r: Some(30.0),
        }
    };
    let mut model = column(shape, "明示強度の鋼管", Some(steel_fy));
    model.materials[1].fc = Some(30.0);
    model.materials[1].name = "Fc30".into();
    model.materials[1].fy = concrete_fy;
    model.elements[0].force_regime = ForceRegime::UniaxialBendingShear;
    model.nodes[1].mass = Some([1000.0, 1000.0, 1000.0, 0.0, 0.0, 0.0]);
    model
}

fn issue1990_pushover(
    label: &str,
    model: &Model,
    control: PushoverControl,
    moment_ref_knm: f64,
    target_mm: f64,
) -> Vec<Sample> {
    let mut model = model.clone();
    model.stories[1].seismic_weight = Some(moment_ref_knm * 1e6 / HEIGHT_MM / 0.2);
    let map = DofMap::build(&model);
    let reducer = Reducer::build(&model, &map);
    let result = pushover_analysis_recording(
        &model,
        &map,
        &reducer,
        SeismicDir::X,
        100,
        PushoverTarget::from_max_disp(target_mm),
        control,
        false,
        Default::default(),
    )
    .unwrap();
    let Some(detail) = result.member_history.iter().find(|m| m.elem == ElemId(0)) else {
        println!(
            "{label} static: {:?}, no detail, last={:?}",
            result.termination,
            result.steps.last()
        );
        return vec![];
    };
    assert_eq!(detail.records.len(), result.steps.len());
    let max_n = detail.records.iter().map(|r| r.n.abs()).fold(0.0, f32::max);
    let max_mz_j = detail
        .records
        .iter()
        .map(|r| r.mz_j.abs())
        .fold(0.0, f32::max);
    let max_my_i = detail
        .records
        .iter()
        .map(|r| r.my_i.abs())
        .fold(0.0, f32::max);
    let confirmed = result.confirmed_history.as_ref().unwrap();
    assert_eq!(confirmed.len(), result.steps.len());
    let max_qy = confirmed
        .iter()
        .map(|step| {
            step.members
                .iter()
                .find(|member| member.elem == ElemId(0))
                .unwrap()
        })
        .zip(&detail.records)
        .map(|(member, point)| {
            assert!((f64::from(point.mz_i).abs() - member.shear_strong * HEIGHT_MM).abs() < 100.0);
            member.shear_strong
        })
        .fold(0.0, f64::max);
    let first_shear = result
        .shear_yields
        .first()
        .map(|event| (event.step, event.detail.map(|detail| detail.limit_n)));
    println!(
        "{label} static: {:?}, Nabs_max_N={max_n:.7}, Mzjabs_max_Nmm={max_mz_j:.5}, Myiabs_max_Nmm={max_my_i:.5}, Qyiabs_max_N={max_qy:.5}, first_shear={first_shear:?}, shear_events={}, shear_diagnostics={}, fiber_records={}",
        result.termination, result.shear_yields.len(), result.shear_diagnostics.len(), result.fiber_states.len(),
    );
    result
        .steps
        .iter()
        .zip(&detail.records)
        .enumerate()
        .map(|(k, (step, point))| Sample {
            index: k,
            at: step.load_factor,
            ux_mm: step.floor_disp[1][0],
            mz_knm: f64::from(point.mz_i).abs() / 1e6,
            spring_rad: Some(f64::from(point.spring_rz_i).abs()),
        })
        .collect()
}

fn issue1990_history(label: &str, model: &Model) -> Vec<Sample> {
    const STEPS: u32 = 2000;
    let map = DofMap::build(model);
    let reducer = Reducer::build(model, &map);
    let result = nonlinear_time_history_analysis(
        model,
        &map,
        &reducer,
        &GroundMotion {
            dt: 0.01,
            accel_x: (0..=STEPS)
                .map(|k| {
                    let x = f64::from(k) / f64::from(STEPS);
                    -625.0 * x * x
                })
                .collect(),
            accel_y: None,
            accel_theta: None,
        },
        &NewmarkCfg::average_accel(),
        &Damping::MassProportional { h: 0.2, omega: 3.0 },
        DampingAccumulation::default(),
        &[],
        &[],
        NonlinearThCfg {
            apply_long_term: false,
            record_every: Some(1),
            ..NonlinearThCfg::new(35, 1e-8)
        },
    )
    .unwrap();
    assert!(result.nonlinear);
    let recording = result.recording.unwrap();
    assert_eq!(recording.frame_time.len(), STEPS as usize + 1);
    let mut max_n: f64 = 0.0;
    let mut max_my_i: f64 = 0.0;
    let mut max_mz_j: f64 = 0.0;
    let mut max_qy: f64 = 0.0;
    let samples = recording
        .frame_time
        .iter()
        .zip(&recording.node_disp)
        .zip(&recording.member_forces)
        .enumerate()
        .map(|(k, ((&at, disp), forces))| {
            let f = &forces[0].as_ref().unwrap().at;
            assert_eq!([f[0].0, f[1].0], [0.0, 1.0]);
            max_n = max_n.max(f[0].1[0].abs()).max(f[1].1[0].abs());
            max_qy = max_qy.max(f[0].1[1].abs());
            max_my_i = max_my_i.max(f[0].1[4].abs());
            max_mz_j = max_mz_j.max(f[1].1[5].abs());
            Sample {
                index: k,
                at,
                ux_mm: disp[1][0],
                mz_knm: f[0].1[5].abs() / 1e6,
                spring_rad: None,
            }
        })
        .collect();
    println!(
        "{label} history: nonconverged={}, Nabs_max_N={max_n:.7}, Mzjabs_max_Nmm={max_mz_j:.5}, Myiabs_max_Nmm={max_my_i:.5}, Qyiabs_max_N={max_qy:.5}",
        result.non_converged_steps,
    );
    samples
}

fn show_segments(samples: &[Sample]) {
    if samples.is_empty() {
        return;
    }
    let points: Vec<_> = samples
        .iter()
        .copied()
        .take_while(|p| p.index == 0 || p.ux_mm > samples[p.index - 1].ux_mm)
        .collect();
    let secant = |a: Sample, b: Sample| (b.mz_knm - a.mz_knm) / (b.ux_mm - a.ux_mm);
    let mut previous: Option<f64> = None;
    for (k, ab) in points.windows(2).enumerate() {
        let [a, b] = [ab[0], ab[1]];
        if a.ux_mm < 0.1 {
            continue;
        }
        let slope = secant(a, b);
        if previous.is_none_or(|s| (slope - s).abs() > 0.05 * s.abs().max(0.02)) {
            println!("secant={slope:.8}  {a} -> {b}");
            if k >= 5 && k + 3 < points.len() && previous.is_some() {
                let left = secant(points[k - 5], points[k - 1]);
                let right = secant(points[k + 2], points[(k + 6).min(points.len() - 1)]);
                if (left - right).abs() > 0.05 * left.abs().max(0.02) {
                    let h = points[k - 1];
                    let j = points[k + 2];
                    let ux = ((j.mz_knm - right * j.ux_mm) - (h.mz_knm - left * h.ux_mm))
                        / (left - right);
                    let mz = h.mz_knm + left * (ux - h.ux_mm);
                    println!("  fitted Ux={ux:.6} Mz={mz:.6}, slopes={left:.8}->{right:.8}");
                }
            }
        }
        previous = Some(slope);
    }
    println!(
        "points={}, initial_loading={}, last={}, final={}",
        samples.len(),
        points.len(),
        points.last().unwrap(),
        samples.last().unwrap(),
    );
}

#[test]
fn issue1990_cft_spring_public_probe() {
    for (is_round, shape) in [(false, "box"), (true, "pipe")] {
        let mut baseline = None;
        for (variant, thickness_mm, steel_fy, concrete_fy) in [
            ("base", 12.0, 235.0, None),
            ("steel", 12.0, 325.0, None),
            ("thick", 16.0, 235.0, None),
            ("concrete", 12.0, 235.0, Some(120.0)),
        ] {
            let label = format!("{shape}/{variant}");
            let model = issue1990_model(is_round, thickness_mm, steel_fy, concrete_fy);
            if concrete_fy.is_some() {
                println!("{label}/LoadOnly/80/350");
                show_segments(&issue1990_pushover(
                    &label,
                    &model,
                    PushoverControl::LoadOnly,
                    80.0,
                    350.0,
                ));
            }
            let (ref_knm, target_mm) = match (is_round, variant) {
                (false, "concrete") | (true, "thick") => (100.0, 660.0),
                _ => (120.0, 500.0),
            };
            println!("{label}/Phased/{ref_knm}/{target_mm}");
            let static_samples =
                issue1990_pushover(&label, &model, PushoverControl::Phased, ref_knm, target_mm);
            show_segments(&static_samples);
            let dynamic_samples = issue1990_history(&label, &model);
            show_segments(&dynamic_samples);
            if variant == "base" {
                baseline = Some((static_samples, dynamic_samples));
            } else if variant == "steel" {
                let (base_static, base_dynamic) = baseline.as_ref().unwrap();
                println!(
                    "{shape}/steel vs base: all static samples equal={}, all dynamic samples equal={}",
                    *base_static == static_samples, *base_dynamic == dynamic_samples,
                );
            }
        }
    }
}
