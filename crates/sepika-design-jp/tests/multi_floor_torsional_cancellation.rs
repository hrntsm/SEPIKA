use approx::assert_relative_eq;
use sepika_core::dof::Dof;
use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, NodeId, SectionId};
use sepika_core::model::{
    Constraint, ElementKind, GirderTorsionMode, LoadCase, LoadCaseKind, Model, NodalLoad, Section,
};
use sepika_design_jp::secondary::eccentricity_analysis::{
    column_horizontal_stiffnesses, story_eccentricity_from_analysis,
};
use sepika_design_jp::secondary::holding_capacity::{eccentricity_ratio, fes, stiffness_ratios};
use sepika_design_jp::secondary::stiffness_ratio::cog_story_drifts;
use sepika_solver::statics::analysis::SeismicDir;
use sepika_solver::statics::linear::{linear_static_once, StaticOnce};

#[allow(dead_code)]
#[path = "fixtures/eccentricity_model.rs"]
mod eccentricity_model;
use eccentricity_model::{column_model, element_nodal_residual, TOP};

const HEADS: [NodeId; 4] = [NodeId(1), NodeId(3), NodeId(5), NodeId(7)];
const XY_MM: [[f64; 2]; 4] = [[0.0, 0.0], [2000.0, 0.0], [0.0, 4000.0], [2000.0, 4000.0]];
const UNIFORM_MM: f64 = 0.25;
const SMALL_SCALE: f64 = 0.001;
type HeadForces = [[f64; 2]; 4];

#[derive(Clone, Copy, Debug)]
enum Floor {
    TwoDiaphragms,
    Shell,
}

fn model_with_floor(floor: Floor) -> Model {
    let mut model = column_model(&XY_MM, 0.0, 0.0);
    model.girder_torsion = GirderTorsionMode::Keep;
    model.sections[0].iy = 12_000_000.0;
    for (id, direction) in HEADS
        .into_iter()
        .zip([[1.0, 0.0], [1.0, 0.0], [1.0, -1.0], [1.0, 1.0]])
    {
        let head = &mut model.nodes[id.index()];
        head.mass = Some([1.0, 1.0, 0.0, 0.0, 0.0, 0.0]);
        for dof in [Dof::Uz, Dof::Rx, Dof::Ry] {
            head.restraint.set_fixed(dof);
        }
        let [x, y] = direction;
        model.elements[id.index() / 2].local_axis.ref_vector = [x, y, 0.0];
    }
    match floor {
        Floor::TwoDiaphragms => {
            for [master, slave] in [[HEADS[0], HEADS[2]], [HEADS[1], HEADS[3]]] {
                model
                    .constraints
                    .push(Constraint::rigid_diaphragm(TOP, master, vec![slave]));
            }
        }
        Floor::Shell => {
            let mut material = model.materials[0].clone();
            material.id = MaterialId(1);
            material.name = "床材".into();
            material.young = 2_000.0;
            model.materials.push(material);
            let mut section = Section::zero(SectionId(1), "床断面".into());
            section.material = Some(MaterialId(1));
            section.thickness = Some(2.0);
            model.sections.push(section);
            let mut shell = model.elements[0].clone();
            shell.id = ElemId(4);
            shell.kind = ElementKind::Shell;
            shell.nodes = [HEADS[0], HEADS[1], HEADS[3], HEADS[2]]
                .into_iter()
                .collect();
            shell.section = Some(SectionId(1));
            model.elements.push(shell);
        }
    }
    model
}

fn uniform_displacement_reactions() -> [HeadForces; 2] {
    // 曲げ・せん断のひずみエネルギーから得る両端固定梁の柔度。床の一様並進では床ひずみが0。
    let stiffness = |i_mm4: f64| {
        let length_mm: f64 = 3000.0;
        1.0 / (length_mm.powi(3) / (12.0 * 200_000.0 * i_mm4) + length_mm / (80_000.0 * 1_600.0))
    };
    let strong = stiffness(12_000_000.0);
    let weak = stiffness(8_000_000.0);
    let diagonal = (strong + weak) / 2.0;
    let cross = (strong - weak) / 2.0;
    [
        [
            [strong, 0.0],
            [strong, 0.0],
            [diagonal, -cross],
            [diagonal, cross],
        ],
        [
            [0.0, weak],
            [0.0, weak],
            [-cross, diagonal],
            [cross, diagonal],
        ],
    ]
    .map(|forces| scale(forces, UNIFORM_MM))
}

fn scale(forces: HeadForces, factor: f64) -> HeadForces {
    forces.map(|pair| pair.map(|value| value * factor))
}

fn solve(model: &mut Model, name: &str, forces: HeadForces) -> StaticOnce {
    let id = LoadCaseId(model.load_cases.len() as u32);
    model.load_cases.push(LoadCase {
        origin: Default::default(),
        id,
        name: name.into(),
        kind: LoadCaseKind::Seismic,
        nodal: HEADS
            .into_iter()
            .zip(forces)
            .map(|(node, [fx, fy])| NodalLoad::manual(node, [fx, fy, 0.0, 0.0, 0.0, 0.0]))
            .collect(),
        member: Vec::new(),
    });
    linear_static_once(model, id).unwrap()
}

fn check_equilibrium(model: &Model, response: &StaticOnce, applied: HeadForces) -> HeadForces {
    let mut external = [0.0; 3];
    let mut columns = [0.0; 3];
    let forces = std::array::from_fn(|index| {
        let (id, recorded) = &response.column_end_horizontal_forces[index];
        let element = &model.elements[index];
        assert_eq!(*id, element.id);
        let [qx, qy] = recorded.as_ref().unwrap()[1];
        let residual = element_nodal_residual(model, element, response, &[]);
        assert_relative_eq!(qx, residual[6], epsilon = 1e-9);
        assert_relative_eq!(qy, residual[7], epsilon = 1e-9);
        let [x_mm, y_mm] = XY_MM[index];
        let [fx, fy] = applied[index];
        external[0] += fx;
        external[1] += fy;
        external[2] += x_mm * fy - y_mm * fx;
        columns[0] += qx;
        columns[1] += qy;
        columns[2] += x_mm * qy - y_mm * qx + residual[11];
        [qx, qy]
    });
    for (actual, expected) in columns.into_iter().zip(external) {
        assert_relative_eq!(actual, expected, epsilon = 1e-8, max_relative = 1e-11);
    }
    let shears = response.story_shears.as_ref().unwrap();
    for axis in 0..2 {
        assert_relative_eq!(shears[axis][0], external[axis], epsilon = 1e-8);
    }
    eprintln!("  層 Fx/Fy/Mz [N, N, N·mm] = {external:.9?} / 柱集計 = {columns:.9?}");
    forces
}

fn check_uniform(model: &Model, response: &StaticOnce, reactions: HeadForces, axis: usize) {
    let forces = check_equilibrium(model, response, reactions);
    for (head, (actual, expected)) in HEADS.into_iter().zip(forces.into_iter().zip(reactions)) {
        let mut expected_disp = [0.0; 6];
        expected_disp[axis] = UNIFORM_MM;
        for (actual, expected) in response.disp[head.index()].into_iter().zip(expected_disp) {
            assert_relative_eq!(actual, expected, epsilon = 1e-11);
        }
        for (actual, expected) in actual.into_iter().zip(expected) {
            assert_relative_eq!(actual, expected, epsilon = 1e-8);
        }
    }
}

fn reproduce(floor: Floor, required_diagnostic: &str) {
    let mut model = model_with_floor(floor);
    let [uniform_x, uniform_y] = uniform_displacement_reactions();
    eprintln!("{floor:?} 一様独立反力 X={uniform_x:.9?}, Y={uniform_y:.9?}");
    let x = solve(&mut model, "X対照", uniform_x);
    let y = solve(&mut model, "Y対照", uniform_y);
    check_uniform(&model, &x, uniform_x, 0);
    check_uniform(&model, &y, uniform_y, 1);

    let couple = solve(
        &mut model,
        "単位偶力",
        [[1.0, 0.0], [0.0; 2], [-1.0, 0.0], [0.0; 2]],
    );
    let couple_n = (SMALL_SCALE - 1.0) * UNIFORM_MM / couple.disp[HEADS[2].index()][0];
    let mut twisted_x = uniform_x;
    twisted_x[0][0] += couple_n;
    twisted_x[2][0] -= couple_n;
    eprintln!("  X偶力片側荷重 [N] = {couple_n:.9}");
    let twisted = solve(&mut model, "Xねじれ", twisted_x);
    let small_x = scale(uniform_x, SMALL_SCALE);
    let small_twisted_x = scale(twisted_x, SMALL_SCALE);
    let small_y = solve(&mut model, "Y対照縮小", scale(uniform_y, SMALL_SCALE));
    let small = solve(&mut model, "X対照縮小", small_x);
    let small_twisted = solve(&mut model, "Xねじれ縮小", small_twisted_x);

    let sample_disp = |response: &StaticOnce| {
        HEADS.map(|id| {
            let [ux, uy, _, _, _, rz] = response.disp[id.index()];
            [ux, uy, rz]
        })
    };
    let d_twisted = sample_disp(&twisted);
    match floor {
        Floor::TwoDiaphragms => {
            for (master, slave) in [(0, 2), (1, 3)] {
                assert_relative_eq!(d_twisted[master][2], d_twisted[slave][2], epsilon = 1e-13);
                assert_relative_eq!(
                    d_twisted[slave][0] - d_twisted[master][0],
                    -4000.0 * d_twisted[master][2],
                    epsilon = 1e-11,
                );
            }
            assert!((d_twisted[0][2] - d_twisted[1][2]).abs() > 1e-5);
        }
        Floor::Shell => {
            // 同じY座標の辺の伸縮は共通の並進・回転で表せない。
            assert!((d_twisted[1][0] - d_twisted[0][0]).abs() > 1e-4);
        }
    }
    let c = 2;
    assert_relative_eq!(d_twisted[c][0], sample_disp(&small)[c][0], epsilon = 1e-11);
    let stiffness_x =
        |response| column_horizontal_stiffnesses(&model, TOP, response, SeismicDir::X).unwrap();
    let control_k = stiffness_x(&x);
    let twisted_k = stiffness_x(&twisted);
    assert_relative_eq!(control_k[c], uniform_x[c][0] / UNIFORM_MM, epsilon = 1e-8);
    assert!(twisted_k[c] > 2.0 * control_k[c]);

    let cases = [
        ("X対照", &x, uniform_x, &y),
        ("X対照縮小", &small, small_x, &small_y),
        ("Xねじれ", &twisted, twisted_x, &y),
        ("Xねじれ縮小", &small_twisted, small_twisted_x, &small_y),
    ];
    for (name, response, loads, response_y) in cases {
        eprintln!("{floor:?} {name} Fx/Fy [N]={loads:.9?}");
        let forces = check_equilibrium(&model, response, loads);
        let displacements = sample_disp(response);
        let [dx, dy, _] = displacements[c];
        assert_relative_eq!(
            forces[c][0],
            uniform_x[c][0] * (dx / UNIFORM_MM) + uniform_y[c][0] * (dy / UNIFORM_MM),
            epsilon = 1e-9,
        );
        let drift_x = cog_story_drifts(&model, &response.disp, 0).unwrap();
        let rs_x = stiffness_ratios(&[3000.0], &drift_x).unwrap()[0];
        let re = story_eccentricity_from_analysis(&model, TOP, response, response_y, None);
        let re_x = re.and_then(|ecc| eccentricity_ratio(ecc.ey, ecc.rex));
        let fes_x = re_x.clone().and_then(|re_x| fes(rs_x, re_x));
        eprintln!("  δx/δy/Rz [mm/mm/rad]={displacements:.12?}\n  Qx/Qy [N]={forces:.9?}");
        eprintln!(
            "  X柱比 [N/mm]={:.6?}; 層重心δx [mm]={drift_x:.9?}; Rs={rs_x}; Re={re_x:?}; Fes={fes_x:?}",
            stiffness_x(response)
        );
        let reason = re_x.unwrap_err();
        assert!(reason.contains(required_diagnostic), "{reason}");
        assert!(reason.contains("部材2の方向別断面剛性の軸"), "{reason}");
        assert_eq!(fes_x, Err(reason));
        assert_eq!(rs_x, 1.0);
    }

    for (large, reduced) in [(&x, &small), (&twisted, &small_twisted)] {
        for (big, tiny) in sample_disp(large)
            .into_iter()
            .flatten()
            .zip(sample_disp(reduced).into_iter().flatten())
        {
            assert_relative_eq!(big * SMALL_SCALE, tiny, epsilon = 1e-12);
        }
        for (large, reduced) in large
            .column_end_horizontal_forces
            .iter()
            .zip(&reduced.column_end_horizontal_forces)
        {
            for (big, tiny) in large.1.as_ref().unwrap()[1]
                .into_iter()
                .zip(reduced.1.as_ref().unwrap()[1])
            {
                assert_relative_eq!(big * SMALL_SCALE, tiny, epsilon = 1e-9);
            }
        }
        for (big, tiny) in stiffness_x(large).into_iter().zip(stiffness_x(reduced)) {
            assert_relative_eq!(big, tiny, epsilon = 1e-7);
        }
    }
}

#[test]
fn two_diaphragms_allow_local_cancellation_without_a_shared_story_rotation() {
    reproduce(Floor::TwoDiaphragms, "層上端に剛床が複数あります");
}

#[test]
fn a_deforming_floor_allows_local_cancellation_without_a_rigid_story_motion() {
    reproduce(Floor::Shell, "層上端に単一剛床がありません");
}
