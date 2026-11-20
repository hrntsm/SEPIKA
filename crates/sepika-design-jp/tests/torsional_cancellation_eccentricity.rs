use approx::assert_relative_eq;
use sepika_core::dof::{Dof, Dof6Mask};
use sepika_core::ids::NodeId;
use sepika_core::model::{Constraint, Node};
use sepika_design_jp::secondary::eccentricity::center_of_rigidity;
use sepika_design_jp::secondary::eccentricity_analysis::{
    column_stiffnesses_from_analysis, story_eccentricity_from_analysis,
};
use sepika_solver::statics::linear::linear_static_once;

#[path = "fixtures/eccentricity_model.rs"]
mod eccentricity_model;
use eccentricity_model::{add_case, column_model, element_nodal_residual, TOP};

#[test]
fn near_cancellation_retains_column_shear_unlike_proportionally_small_translation() {
    let positions_mm = [
        [0.0, 0.0],
        [2000.0, 0.0],
        [1000.0, -4000.0],
        [1000.0, 4000.0],
    ];
    let mut model = column_model(&positions_mm, 0.0, 0.0);
    model.sections[0].iy = 12_000_000.0;
    for (element, ref_vector) in model.elements.iter_mut().zip([
        [1.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, -1.0, 0.0],
        [1.0, 1.0, 0.0],
    ]) {
        element.local_axis.ref_vector = ref_vector;
    }
    let gravity_x_mm = 565.217_391_304_348;
    let gravity_y_mm = 1_900.137_459_724_38;
    let mass_node = NodeId(8);
    model.nodes.push(Node {
        id: mass_node,
        coord: [gravity_x_mm, gravity_y_mm, 3000.0],
        restraint: Dof6Mask::FREE,
        mass: Some([1.0, 1.0, 0.0, 0.0, 0.0, 0.0]),
        story: Some(TOP),
        support_spring: None,
    });
    model.stories[TOP.index()].node_ids.push(mass_node);
    for head in model.nodes.iter_mut().filter(|n| n.story == Some(TOP)) {
        for dof in [Dof::Uz, Dof::Rx, Dof::Ry] {
            head.restraint.set_fixed(dof);
        }
    }
    model.constraints.push(Constraint::rigid_diaphragm(
        TOP,
        NodeId(1),
        vec![NodeId(3), NodeId(5), NodeId(7), mass_node],
    ));

    let loads = [
        [1000.0, 0.0, 0.0, 0.0, 0.0, -1000.0 * gravity_y_mm],
        [1000.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.0, 1000.0, 0.0, 0.0, 0.0, 1000.0 * gravity_x_mm],
        [0.0, 1.0, 0.0, 0.0, 0.0, gravity_x_mm],
        [0.0, 0.0, 0.0, 0.0, 0.0, 1_000_000.0],
    ];
    let names = [
        "重心X",
        "純並進X",
        "小荷重X",
        "重心Y",
        "小荷重Y",
        "純ねじり",
    ];
    let [x, pure_x, small_x, y, small_y, twist] = std::array::from_fn(|index| {
        let case = add_case(&mut model, names[index], loads[index]);
        linear_static_once(&model, case).unwrap()
    });

    for (response, load) in [&x, &pure_x, &small_x, &y, &small_y, &twist]
        .into_iter()
        .zip(loads)
    {
        let theta = response.disp[1][5];
        let mut moment_at_origin = 0.0;
        for (element, [x_mm, y_mm]) in model.elements.iter().zip(positions_mm) {
            let head = element_nodal_residual(&model, element, response, &[]);
            let (id, end_forces) = &response.column_end_horizontal_forces[element.id.index()];
            assert_eq!(*id, element.id);
            let upper_force = end_forces.as_ref().unwrap()[1];
            assert_relative_eq!(upper_force[0], head[6], epsilon = 1e-9);
            assert_relative_eq!(upper_force[1], head[7], epsilon = 1e-9);
            assert_relative_eq!(head[11], 106_666_666.666_667 * theta, epsilon = 1e-8);
            moment_at_origin += x_mm * head[7] - y_mm * head[6] + head[11];
        }
        assert_relative_eq!(moment_at_origin, load[5], epsilon = 1e-7);
        let shears = response.story_shears.as_ref().unwrap();
        assert_relative_eq!(shears[0][0], load[0], epsilon = 1e-9);
        assert_relative_eq!(shears[1][0], load[1], epsilon = 1e-9);
    }

    // 両端回転を拘束した一様梁の曲げ・せん断柔度と剛床の釣合いによる独立値。
    let [ux, uy, _, _, _, theta] = x.disp[1];
    assert_relative_eq!(ux, 0.261_683_872_767_857, epsilon = 1e-12);
    assert_relative_eq!(uy, 0.036_940_091_909_088_7, epsilon = 1e-12);
    assert_relative_eq!(theta, -6.535_554_722_377_23e-5, epsilon = 1e-14);
    assert_relative_eq!(x.disp[5][0], ux + 4000.0 * theta, epsilon = 1e-12);
    assert_relative_eq!(x.disp[5][1], -0.028_415_455_314_683_6, epsilon = 1e-12);
    assert_relative_eq!(x.disp[5][0], small_x.disp[5][0], epsilon = 1e-12);

    let target = &model.elements[2];
    let head_x = element_nodal_residual(&model, target, &x, &[]);
    let head_small_x = element_nodal_residual(&model, target, &small_x, &[]);
    assert_relative_eq!(head_x[6], 5.075_310_559_006_21, epsilon = 1e-9);
    assert_relative_eq!(head_x[7], -24.767_565_993_788_8, epsilon = 1e-9);
    assert_relative_eq!(head_small_x[6], 0.227_678_571_428_571, epsilon = 1e-9);
    assert_relative_eq!(head_small_x[7], -0.044_642_857_142_857_1, epsilon = 1e-9);
    eprintln!(
        "対象柱CのX並進・回転変位 [mm] = {ux:.12}, {:.12}",
        4000.0 * theta
    );
    for (name, response) in [("重心X", &x), ("純並進X", &pure_x), ("小荷重X", &small_x)] {
        eprintln!("{name}の柱変位差 [mm] と符号付き柱頭力 [N]");
        for element in &model.elements {
            let disp = response.disp[element.nodes[1].index()];
            let head = element_nodal_residual(&model, element, response, &[]);
            eprintln!(
                "  δx={:.12}, δy={:.12}, Qx={:.9}, Qy={:.9}",
                disp[0], disp[1], head[6], head[7]
            );
        }
    }

    for (response, direction, expected) in [
        (&pure_x, 0, 3_821.404_771_424_76),
        (&y, 1, 3_139.011_062_241_77),
    ] {
        assert_relative_eq!(
            1000.0 / response.disp[1][direction],
            expected,
            epsilon = 1e-8
        );
        assert_relative_eq!(response.disp[1][1 - direction], 0.0, epsilon = 1e-12);
        assert_relative_eq!(response.disp[1][5], 0.0, epsilon = 1e-14);
    }
    let [u, v, _, _, _, r] = twist.disp[1];
    let rigidity_center = [-v / r, u / r];
    let torsional_stiffness = 1_000_000.0 / r;
    assert_relative_eq!(rigidity_center[0], 565.217_391_304_348, epsilon = 1e-8);
    assert_relative_eq!(rigidity_center[1], 0.0, epsilon = 1e-8);
    assert_relative_eq!(
        torsional_stiffness,
        29_073_851_271.085_8,
        max_relative = 1e-12
    );
    let independent_x = gravity_y_mm / (torsional_stiffness / (1000.0 / pure_x.disp[1][0])).sqrt();
    assert_relative_eq!(independent_x, 0.688_882_668_214_38, epsilon = 1e-12);

    let pure = story_eccentricity_from_analysis(&model, TOP, &pure_x, &y, None).unwrap();
    let small = story_eccentricity_from_analysis(&model, TOP, &small_x, &small_y, None).unwrap();
    for (a, b) in [
        (*pure.re_x.as_ref().unwrap(), *small.re_x.as_ref().unwrap()),
        (*pure.re_y.as_ref().unwrap(), *small.re_y.as_ref().unwrap()),
        (pure.kr, small.kr),
    ] {
        assert_relative_eq!(a, b, max_relative = 1e-10);
    }
    let pure_cols = column_stiffnesses_from_analysis(&model, TOP, &pure_x, &y).unwrap();
    let small_cols = column_stiffnesses_from_analysis(&model, TOP, &small_x, &small_y).unwrap();
    for (normal, scaled) in pure_cols.iter().zip(&small_cols) {
        assert_relative_eq!(normal.dx, scaled.dx, epsilon = 1e-8);
        assert_relative_eq!(normal.dy, scaled.dy, epsilon = 1e-8);
    }
    let current = story_eccentricity_from_analysis(&model, TOP, &x, &y, None);
    let x_cols = column_stiffnesses_from_analysis(&model, TOP, &x, &y);
    for (name, result, columns) in [
        ("重心加力の公開結果", current, x_cols),
        ("純並進の公開結果", Ok(pure), Ok(pure_cols)),
        ("小荷重の公開結果", Ok(small), Ok(small_cols)),
    ] {
        match (result, columns) {
            (Ok(re), Ok(cols)) => eprintln!(
                "{name}の剛心={:?}, KR={:.6}, Re=({:?}, {:?}), 柱Ki={cols:?}",
                center_of_rigidity(&cols).unwrap(),
                re.kr,
                re.re_x,
                re.re_y
            ),
            (result, columns) => eprintln!("{name}: {result:?}, {columns:?}"),
        }
    }
}
