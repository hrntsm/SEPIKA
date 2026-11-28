use sepika_core::dof::{Dof6Mask, DofMap};
use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId};
use sepika_core::model::*;
use sepika_core::section_shape::SectionShape;
use sepika_solver::common::constraint::Reducer;
use sepika_solver::dynamic::damping::Damping;
use sepika_solver::dynamic::timehistory::{
    linear_time_history_analysis, linear_time_history_until_with_force_observer, GroundMotion,
    NewmarkCfg,
};

const UNDAMPED: Damping = Damping::MassProportional { h: 0.0, omega: 1.0 };

fn axial_beam() -> Model {
    let mut section = SectionShape::SteelFlatBar {
        width: 1.0,
        thick: 1.0,
    }
    .to_section(SectionId(0), "PL-1×1".into());
    section.material = Some(MaterialId(0));
    Model {
        nodes: vec![
            Node {
                id: NodeId(0),
                coord: [0.0; 3],
                restraint: Dof6Mask::FIXED,
                mass: None,
                story: None,
                support_spring: None,
            },
            Node {
                id: NodeId(1),
                coord: [1000.0, 0.0, 0.0],
                restraint: Dof6Mask(0b111110),
                mass: Some([10.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
                story: None,
                support_spring: None,
            },
        ],
        elements: vec![ElementData {
            id: ElemId(17),
            kind: ElementKind::Beam,
            nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 0.0, 1.0],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            end_offsets_mm: [[0.0; 3]; 2],
            plastic_zone: None,
            spring: None,
        }],
        sections: vec![section],
        materials: vec![Material {
            id: MaterialId(0),
            name: "Steel".into(),
            category: MaterialCategory::Steel,
            young: 1_000_000.0,
            poisson: 0.3,
            density: 0.0,
            shear: None,
            fc: None,
            fy: Some(235.0),
            strength_factor: None,
            concrete_class: Default::default(),
        }],
        mass_method: MassMethod::LumpedOnly,
        ..Default::default()
    }
}

fn wave(accel_x: Vec<f64>) -> GroundMotion {
    GroundMotion {
        dt: 0.02,
        accel_x,
        accel_y: None,
        accel_theta: None,
    }
}

#[test]
fn linear_time_history_rejects_overflow_between_saved_frames() {
    let model = axial_beam();
    let dofmap = DofMap::build(&model);
    let reducer = Reducer::build(&model, &dofmap);
    assert_eq!(reducer.n_indep, 1);

    for record_every in [Some(1), Some(3)] {
        let error = linear_time_history_analysis(
            &model,
            &dofmap,
            &reducer,
            &wave(vec![0.0, 2.0, 1e308, 0.0]),
            &NewmarkCfg::average_accel(),
            &UNDAMPED,
            &[0.0],
            &[0.0],
            record_every,
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("step 2") && error.contains("時刻 0.04 s"),
            "{error}"
        );
        assert!(error.contains("積分応答に非有限値"), "{error}");
    }
}

#[test]
fn finite_linear_time_history_preserves_axial_history_and_envelope() {
    let model = axial_beam();
    let dofmap = DofMap::build(&model);
    let reducer = Reducer::build(&model, &dofmap);
    let wave = wave(vec![0.0, 2.0, 100.0, 0.0]);
    let time_s = [0.0, 0.02, 0.04, 0.06];
    // 1 自由度の運動方程式 a + 100u = −a_g と台形則による相対変位。
    let disp_mm = [
        0.0,
        -0.000198019801980198,
        -0.01068522693853544,
        -0.04074925677059423,
    ];
    let axial_force_n = [
        0.0,
        -0.198019801980198,
        -10.68522693853544,
        -40.74925677059423,
    ];

    for (record_every, steps) in [(1, &[0, 1, 2, 3][..]), (3, &[0, 3][..])] {
        let result = linear_time_history_analysis(
            &model,
            &dofmap,
            &reducer,
            &wave,
            &NewmarkCfg::average_accel(),
            &UNDAMPED,
            &[0.0],
            &[0.0],
            Some(record_every),
        )
        .unwrap();
        assert_eq!(result.time, time_s);
        assert_eq!(result.history.node, Some(NodeId(1)));
        assert_eq!(result.history.node_disp.len(), time_s.len());
        for (&actual, expected) in result.history.node_disp.iter().zip(disp_mm) {
            assert!((actual - expected).abs() < 1e-12, "{actual} ≠ {expected}");
        }
        assert!((result.peak_disp[1][0] - 0.04074925677059423).abs() < 1e-12);

        let recording = result.recording.unwrap();
        assert_eq!(
            recording.frame_time,
            steps.iter().map(|&step| time_s[step]).collect::<Vec<_>>()
        );
        for (frame, &step) in steps.iter().enumerate() {
            let stations = &recording.member_forces[frame][0].as_ref().unwrap().at;
            assert_eq!(stations.len(), 2);
            for (_, forces) in stations {
                assert!((forces[0] - axial_force_n[step]).abs() < 1e-10);
                assert_eq!(forces[1..], [0.0; 5]);
            }
        }
        let envelope = &recording.peak_member_forces[0].as_ref().unwrap().at;
        assert_eq!(envelope.len(), 3);
        for (_, forces) in envelope {
            assert!((forces[0] - (-40.74925677059423)).abs() < 1e-10);
            assert_eq!(forces[1..], [0.0; 5]);
        }
    }
}

#[test]
fn linear_time_history_rejects_nonfinite_initial_section_forces_before_observation() {
    let mut model = axial_beam();
    model.nodes[1].coord = [0.0, 0.0, 1000.0];
    model.elements[0].local_axis.ref_vector = [1.0, 0.0, 0.0];
    let mut section = SectionShape::SteelFlatBar {
        width: 100.0,
        thick: 100.0,
    }
    .to_section(SectionId(0), "PL-100×100".into());
    section.material = Some(MaterialId(0));
    model.sections[0] = section;
    let dofmap = DofMap::build(&model);
    let reducer = Reducer::build(&model, &dofmap);

    let error = linear_time_history_until_with_force_observer(
        &model,
        &dofmap,
        &reducer,
        &wave(vec![0.0]),
        &NewmarkCfg::average_accel(),
        &UNDAMPED,
        &[1e301],
        &[0.0],
        Some(3),
        None,
        Some(&mut |_| panic!("不正な初期状態を観測した")),
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("step 0") && error.contains("時刻 0 s"),
        "{error}"
    );
    assert!(
        error.contains("要素 ID 17") && error.contains("断面力に非有限値"),
        "{error}"
    );
}
