use approx::assert_relative_eq;
use sepika_core::dof::Dof;
use sepika_core::ids::{ElemId, NodeId};
use sepika_core::model::{Constraint, MemberLoad, MemberLoadKind, NodalLoad, ZoneSource};
use sepika_design_jp::secondary::eccentricity::{
    center_of_rigidity, eccentricity, ColumnStiffness,
};
use sepika_design_jp::secondary::eccentricity_analysis::{
    column_horizontal_stiffnesses, column_stiffnesses_from_analysis,
    story_eccentricity_from_analysis,
};
use sepika_design_jp::secondary::holding_capacity::fes;
use sepika_solver::statics::analysis::SeismicDir;
use sepika_solver::statics::linear::{linear_static_once, superpose_static};
use smallvec::smallvec;

#[path = "fixtures/eccentricity_model.rs"]
mod eccentricity_model;
use eccentricity_model::{add_case, column_model, element_nodal_residual, TOP};

#[test]
fn shared_upper_sl_counts_every_independent_column_in_both_directions() {
    let span_mm = 6000.0;
    let mut model = column_model(&[[0.0, 0.0], [span_mm, 0.0], [0.0, span_mm]], 4000.0, 0.0);
    let mut mirror_base = model.nodes[0].clone();
    mirror_base.id = NodeId(6);
    mirror_base.coord[0] = 4000.0;
    model.nodes.push(mirror_base);
    model.stories[0].node_ids.push(NodeId(6));
    let mut mirror_column = model.elements[0].clone();
    mirror_column.id = ElemId(3);
    mirror_column.nodes = smallvec![NodeId(1), NodeId(6)];
    model.elements.push(mirror_column);

    let responses = [0, 1].map(|axis| {
        let mut forces = [0.0; 6];
        forces[axis] = 1000.0;
        let case = add_case(&mut model, if axis == 0 { "EX" } else { "EY" }, forces);
        forces[axis] = 500.0;
        for node in [NodeId(3), NodeId(5)] {
            model.load_cases[case.index()]
                .nodal
                .push(NodalLoad::manual(node, forces));
        }
        linear_static_once(&model, case).unwrap()
    });

    // 共有柱頭の1000 Nを対称な2本が等分し、独立した鉛直柱の各柱頭には500 Nが作用する。
    for (axis, response) in responses.iter().enumerate() {
        let sum_forces: f64 = response
            .column_end_horizontal_forces
            .iter()
            .map(|(id, forces)| {
                let member = model.element(*id).unwrap();
                let end = usize::from(member.nodes[0] != NodeId(1));
                let q_n = forces.as_ref().unwrap()[end][axis];
                assert_relative_eq!(q_n, 500.0, epsilon = 1e-8);
                q_n
            })
            .sum();
        assert_relative_eq!(sum_forces, 2000.0, epsilon = 1e-8);
        assert_relative_eq!(
            response.story_shears.as_ref().unwrap()[axis][0],
            2000.0,
            epsilon = 1e-8
        );
    }

    let [x, y] = &responses;
    let columns = column_stiffnesses_from_analysis(&model, TOP, x, y).unwrap();
    let reason =
        sepika_design_jp::secondary::eccentricity::story_eccentricity(&model, TOP).unwrap_err();
    assert!(
        reason.contains("上階SLの節点1を共有する柱のD値略算法は未対応"),
        "{reason}"
    );
    assert_eq!(columns.len(), 4);
    assert_eq!(columns.iter().filter(|c| c.pos == [0.0; 2]).count(), 2);
    for (axis, direction) in [SeismicDir::X, SeismicDir::Y].into_iter().enumerate() {
        let response = &responses[axis];
        let stiffness = column_horizontal_stiffnesses(&model, TOP, response, direction).unwrap();
        let reacted_n: f64 = stiffness
            .iter()
            .zip([1, 1, 3, 5])
            .map(|(ki, head)| ki * response.disp[head][axis].abs())
            .sum();
        assert_relative_eq!(reacted_n, 2000.0, epsilon = 1e-8);
    }

    // 原点以外の同一断面の柱は、X・Yとも同じ剛性 k。三角形の重心は (L/3,L/3)。
    // k の層剛性に対する比を a, b とすると、剛心は (bL,aL)、ねじり剛性は k L²(2−a−b)。
    let [a, b] = [0, 1].map(|axis| {
        let displacement = &responses[axis].disp;
        assert_relative_eq!(displacement[3][axis], displacement[5][axis], epsilon = 1e-8);
        0.5 / (1.0 + displacement[3][axis] / displacement[1][axis])
    });
    assert_relative_eq!(500.0 / x.disp[3][0], 500.0 / y.disp[3][1], epsilon = 1e-8);
    let [cor_x, cor_y] = center_of_rigidity(&columns).unwrap();
    assert_relative_eq!(cor_x, b * span_mm, epsilon = 1e-8);
    assert_relative_eq!(cor_y, a * span_mm, epsilon = 1e-8);
    let torsion_factor = 2.0 - a - b;
    let result = story_eccentricity_from_analysis(&model, TOP, x, y, None).unwrap();
    assert_relative_eq!(
        result.re_x.unwrap(),
        (1.0 / 3.0 - a).abs() / (a * torsion_factor).sqrt(),
        epsilon = 1e-8
    );
    assert_relative_eq!(
        result.re_y.unwrap(),
        (1.0 / 3.0 - b).abs() / (b * torsion_factor).sqrt(),
        epsilon = 1e-8
    );
}

#[test]
fn inclined_ordinary_beam_reactions_and_signed_section_forces_balance_the_applied_load() {
    let component_45 = 1000.0 * std::f64::consts::FRAC_1_SQRT_2;
    for (dx_mm, py_n, pz_n, expected_section) in [
        (0.0, 700.0, -500.0, [-500.0, -700.0, 1000.0]),
        (4000.0, 700.0, -500.0, [500.0, -700.0, 1000.0]),
        (-4000.0, 700.0, -500.0, [-1100.0, -700.0, 200.0]),
        (3000.0, 0.0, 0.0, [component_45, 0.0, component_45]),
    ] {
        for reversed in [false, true] {
            let mut model = column_model(&[[0.0, 0.0]], dx_mm, 0.0);
            if reversed {
                model.elements[0].nodes.swap(0, 1);
            }
            let case = add_case(&mut model, "先端荷重", [1000.0, py_n, pz_n, 0.0, 0.0, 0.0]);
            let response = linear_static_once(&model, case).unwrap();
            let residual = element_nodal_residual(&model, &model.elements[0], &response, &[]);
            let fixed = if reversed { 6 } else { 0 };
            for (actual, expected) in residual[fixed..fixed + 6].iter().zip([
                -1000.0,
                -py_n,
                -pz_n,
                3000.0 * py_n,
                -3_000_000.0 + pz_n * dx_mm,
                -py_n * dx_mm,
            ]) {
                assert_relative_eq!(*actual, expected, epsilon = 1e-6);
            }
            let story_shears = response.story_shears.as_ref().unwrap();
            assert_relative_eq!(story_shears[0][0], 1000.0, epsilon = 1e-8);
            assert_relative_eq!(story_shears[1][0], py_n, epsilon = 1e-8);

            let [n, qy, qz] = expected_section;
            let expected = if reversed { [n, -qy, qz] } else { [n, qy, qz] };
            for (_, section) in &response.member_forces[0].1.at {
                for (actual, expected) in section.iter().zip(expected) {
                    assert_relative_eq!(*actual, expected, epsilon = 1e-8);
                }
            }
            let [(_, signed)] = response.column_end_horizontal_forces.as_slice() else {
                panic!("対象柱の負担水平力が一意に記録されていません");
            };
            let ends = signed.as_ref().unwrap();
            for (axis, (direction, expected)) in [SeismicDir::X, SeismicDir::Y]
                .into_iter()
                .zip([1000.0, py_n])
                .enumerate()
            {
                assert_relative_eq!(ends[usize::from(!reversed)][axis], expected, epsilon = 1e-8);
                assert_relative_eq!(ends[usize::from(reversed)][axis], -expected, epsilon = 1e-8);
                if py_n != 0.0 || axis == 0 {
                    let ki = column_horizontal_stiffnesses(&model, TOP, &response, direction)
                        .unwrap()[0];
                    assert_relative_eq!(
                        ki,
                        expected.abs() / response.disp[1][axis].abs(),
                        max_relative = 1e-9
                    );
                }
            }
        }
    }
}

#[test]
fn inclined_ordinary_column_eccentricity_is_compared_with_the_rigid_floor_response() {
    let mut model = column_model(&[[0.0, 0.0], [6000.0, 0.0], [0.0, 6000.0]], 2400.0, 3200.0);
    for head in model.nodes.iter_mut().skip(1).step_by(2) {
        for dof in [Dof::Uz, Dof::Rx, Dof::Ry] {
            head.restraint.set_fixed(dof);
        }
    }
    model.constraints.push(Constraint::rigid_diaphragm(
        TOP,
        NodeId(1),
        vec![NodeId(3), NodeId(5)],
    ));
    let x = add_case(
        &mut model,
        "重心X",
        [1000.0, 0.0, 0.0, 0.0, 0.0, -2_000_000.0],
    );
    let y = add_case(
        &mut model,
        "重心Y",
        [0.0, 1000.0, 0.0, 0.0, 0.0, 2_000_000.0],
    );
    let torsion = add_case(
        &mut model,
        "純ねじり",
        [0.0, 0.0, 0.0, 0.0, 0.0, 1_000_000.0],
    );
    let [res_x, res_y, res_twist] =
        [x, y, torsion].map(|case| linear_static_once(&model, case).unwrap());
    for (result, expected_xy) in [
        (&res_x, [1000.0, 0.0]),
        (&res_y, [0.0, 1000.0]),
        (&res_twist, [0.0, 0.0]),
    ] {
        for (axis, expected) in expected_xy.into_iter().enumerate() {
            assert_relative_eq!(
                result.story_shears.as_ref().unwrap()[axis][0],
                expected,
                epsilon = 1e-8
            );
        }
    }
    let current = story_eccentricity_from_analysis(&model, TOP, &res_x, &res_y, None).unwrap();
    let current_columns = column_stiffnesses_from_analysis(&model, TOP, &res_x, &res_y).unwrap();
    let current_center = center_of_rigidity(&current_columns).unwrap();
    let center_of_mass = [2000.0, 2000.0];
    let with_nodal_forces: Vec<_> = model
        .elements
        .iter()
        .zip(&current_columns)
        .map(|(element, col)| {
            let head = element.nodes[1].index();
            let fx = element_nodal_residual(&model, element, &res_x, &[])[6];
            let fy = element_nodal_residual(&model, element, &res_y, &[])[7];
            ColumnStiffness {
                pos: col.pos,
                dx: fx.abs() / res_x.disp[head][0].abs(),
                dy: fy.abs() / res_y.disp[head][1].abs(),
            }
        })
        .collect();
    let nodal_center = center_of_rigidity(&with_nodal_forces).unwrap();
    let nodal_ecc = eccentricity(&with_nodal_forces, center_of_mass, nodal_center).unwrap();
    for (actual, expected) in current_columns.iter().zip(&with_nodal_forces) {
        assert_relative_eq!(actual.dx, expected.dx, max_relative = 1e-9);
        assert_relative_eq!(actual.dy, expected.dy, max_relative = 1e-9);
    }
    for (actual, expected) in [
        (current_center[0], nodal_center[0]),
        (current_center[1], nodal_center[1]),
        (current.kr, nodal_ecc.kr),
        (
            *current.re_x.as_ref().unwrap(),
            *nodal_ecc.re_x.as_ref().unwrap(),
        ),
        (
            *current.re_y.as_ref().unwrap(),
            *nodal_ecc.re_y.as_ref().unwrap(),
        ),
    ] {
        assert_relative_eq!(actual, expected, max_relative = 1e-9);
    }

    let [ux, uy, _, _, _, rotation] = res_twist.disp[1];
    let floor_center = [-uy / rotation, ux / rotation];
    let floor_kr = 1_000_000.0 / rotation;
    let [[cxx, cyx], [cxy, cyy]] = [&res_x, &res_y].map(|result| {
        let [u, v, _, _, _, r] = result.disp[1];
        [
            (u - ux * (r / rotation)) / 1000.0,
            (v - uy * (r / rotation)) / 1000.0,
        ]
    });
    assert_relative_eq!(cxy, cyx, epsilon = 1e-14);
    let determinant = cxx * cyy - cxy * cyx;
    let [kxx, kxy, kyy] = [cyy / determinant, -cxy / determinant, cxx / determinant];
    // 柱先端の力・モーメントによるたわみと回転の柔度を先に求め、剛床へ合成した独立値。
    for (actual, expected) in [kxx, kxy, kyy, floor_center[0], floor_center[1], floor_kr]
        .into_iter()
        .zip([
            19_948.412_630_664_1,
            24_529.095_506_958_25,
            34_257.051_676_389_75,
            2_166.687_027_709_256,
            2_862.348_055_979_125,
            31_403_342_934.896_14,
        ])
    {
        assert_relative_eq!(actual, expected, max_relative = 1e-10);
    }
    let floor_re = [
        (center_of_mass[1] - floor_center[1]).abs() / (floor_kr / kxx).sqrt(),
        (center_of_mass[0] - floor_center[0]).abs() / (floor_kr / kyy).sqrt(),
    ];
    let current_re = [current.re_x.unwrap(), current.re_y.unwrap()];
    let nodal_re = [nodal_ecc.re_x.unwrap(), nodal_ecc.re_y.unwrap()];
    eprintln!(
        "X/Y加力時の節点変位差 [mm] = {:?} / {:?}",
        [1, 3, 5].map(|i| res_x.disp[i][0]),
        [1, 3, 5].map(|i| res_y.disp[i][1])
    );
    eprintln!(
        "方向別柱剛性 [N/mm] = {current_columns:?} / {with_nodal_forces:?}、剛床Kxx/Kxy/Kyy = {kxx}/{kxy}/{kyy}"
    );
    for (name, center, kr, re) in [
        ("現行公開Re", current_center, current.kr, current_re),
        ("物理節点力で置換", nodal_center, nodal_ecc.kr, nodal_re),
        ("剛床応答で比較", floor_center, floor_kr, floor_re),
    ] {
        eprintln!(
            "{name} の剛心={center:?}、KR={kr}、Re={re:?}、Fes={:?}",
            re.map(|r| fes(1.0, r).unwrap())
        );
    }
}

#[test]
fn the_upper_inward_cut_accounts_for_end_loads_with_offsets_and_rigid_zones() {
    for reversed in [false, true] {
        let mut model = column_model(&[[0.0, 0.0]], 3000.0, 0.0);
        model.stress_cfg.consider_rigid_zones = true;
        let elem = &mut model.elements[0];
        if reversed {
            elem.nodes.swap(0, 1);
        }
        let upper = usize::from(!reversed);
        elem.end_offsets_mm[upper][0] = 1000.0;
        elem.rigid_zone.length_i = 400.0;
        elem.rigid_zone.length_j = 400.0;
        elem.rigid_zone.source_i = ZoneSource::Manual;
        elem.rigid_zone.source_j = ZoneSource::Manual;
        let case = add_case(
            &mut model,
            "材端荷重・剛域内荷重・分布荷重",
            [1000.0, 700.0, -500.0, 0.0, 0.0, 0.0],
        );
        let from_base = |distance_mm: f64| {
            if reversed {
                5000.0 - distance_mm
            } else {
                distance_mm
            }
        };
        let point = |distance_mm, dir, p| {
            MemberLoad::manual(
                ElemId(0),
                dir,
                MemberLoadKind::Point {
                    a: from_base(distance_mm),
                    p,
                },
            )
        };
        let [a, b] = if reversed {
            [4300.0, 4500.0]
        } else {
            [500.0, 700.0]
        };
        model.load_cases[case.index()].member = vec![
            point(0.0, [1.0, 0.0, 0.0], 50.0),
            point(4850.0, [1.0, 0.0, 0.0], 350.0),
            point(5000.0, [1.0, 0.0, 0.0], -250.0),
            point(5000.0, [0.0, 1.0, 0.0], 150.0),
            MemberLoad::manual(
                ElemId(0),
                [0.0, 1.0, 0.0],
                MemberLoadKind::Distributed {
                    a,
                    b,
                    w1: -1.0,
                    w2: -1.0,
                },
            ),
        ];
        let response = linear_static_once(&model, case).unwrap();
        let ends = response.column_end_horizontal_forces[0].1.as_ref().unwrap();
        for (axis, (direction, expected)) in [SeismicDir::X, SeismicDir::Y]
            .into_iter()
            .zip([750.0, 850.0])
            .enumerate()
        {
            assert_relative_eq!(ends[upper][axis], expected, epsilon = 1e-8);
            let ki = column_horizontal_stiffnesses(&model, TOP, &response, direction).unwrap()[0];
            assert_relative_eq!(
                ki,
                expected / response.disp[1][axis].abs(),
                max_relative = 1e-9
            );
        }
        assert_relative_eq!(ends[1 - upper][0], -1100.0, epsilon = 1e-8);
        assert_relative_eq!(ends[1 - upper][1], -650.0, epsilon = 1e-8);
        let residual = element_nodal_residual(
            &model,
            &model.elements[0],
            &response,
            &model.load_cases[case.index()].member,
        );
        let fixed = (1 - upper) * 6;
        assert_relative_eq!(residual[fixed], -1150.0, epsilon = 1e-8);
        assert_relative_eq!(residual[fixed + 1], -650.0, epsilon = 1e-8);

        let sum = superpose_static(&[(&response, 1.0), (&response, -1.5)]).unwrap();
        let combined = sum.column_end_horizontal_forces[0].1.as_ref().unwrap();
        assert_relative_eq!(combined[upper][0], -375.0, epsilon = 1e-8);
        assert_relative_eq!(combined[upper][1], -425.0, epsilon = 1e-8);
        let mut missing = response.clone();
        missing.column_end_horizontal_forces.clear();
        let sum = superpose_static(&[(&response, 1.0), (&missing, 1.0)]).unwrap();
        let reason =
            story_eccentricity_from_analysis(&model, TOP, &sum, &response, None).unwrap_err();
        assert!(
            reason.contains("柱0") && reason.contains("合成対象ケース"),
            "{reason}"
        );
    }
}

#[test]
fn an_interior_segment_load_does_not_change_the_upper_cut_force() {
    for reversed in [false, true] {
        let mut model = column_model(&[[0.0, 0.0]], -4000.0, 0.0);
        let mut split = model.nodes[1].clone();
        split.id = NodeId(2);
        split.coord = [2000.0, 0.0, 1500.0];
        split.story = None;
        model.nodes.push(split);
        model.elements[0].nodes[1] = NodeId(2);
        let mut upper = model.elements[0].clone();
        upper.id = ElemId(1);
        upper.nodes = smallvec![NodeId(2), NodeId(1)];
        if reversed {
            upper.nodes.swap(0, 1);
        }
        model.elements.push(upper);
        let case = add_case(&mut model, "分割柱", [1000.0, 700.0, 0.0, 0.0, 0.0, 0.0]);
        model.load_cases[case.index()].nodal.push(NodalLoad::manual(
            NodeId(2),
            [-400.0, 200.0, 0.0, 0.0, 0.0, 0.0],
        ));
        let response = linear_static_once(&model, case).unwrap();
        let upper_ends = response.column_end_horizontal_forces[1].1.as_ref().unwrap();
        assert_relative_eq!(
            upper_ends[usize::from(!reversed)][0],
            1000.0,
            epsilon = 1e-8
        );
        assert_relative_eq!(
            response.column_end_horizontal_forces[0].1.as_ref().unwrap()[0][0],
            -600.0,
            epsilon = 1e-8
        );
        let kx = column_horizontal_stiffnesses(&model, TOP, &response, SeismicDir::X).unwrap();
        assert_eq!(kx.len(), 1);
        assert_relative_eq!(
            kx[0],
            1000.0 / response.disp[1][0].abs(),
            max_relative = 1e-9
        );

        for (elem, end, offset, detail) in [
            (1, usize::from(!reversed), -100.0, "上端の実材軸端"),
            (0, 1, 1500.0, "分割部材0"),
        ] {
            model.elements[elem].end_offsets_mm[end][2] = offset;
            let shifted = linear_static_once(&model, case).unwrap();
            let reason = story_eccentricity_from_analysis(&model, TOP, &shifted, &shifted, None)
                .unwrap_err();
            assert!(
                reason.contains("柱1") && reason.contains("切断位置") && reason.contains(detail),
                "{reason}"
            );
            model.elements[elem].end_offsets_mm[end][2] = 0.0;
        }
    }
}
