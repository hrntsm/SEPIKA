use approx::assert_relative_eq;
use sepika_core::dof::{Dof, Dof6Mask};
use sepika_core::ids::{ElemId, NodeId, SectionId, StoryId};
use sepika_core::model::{
    Constraint, ElementKind, EndCondition, FrameSectionUse, MaterialCategory, MemberLoad,
    MemberLoadKind, NodalLoad, Story, WallAttr, WallOpening, WallPlate, WallPlateShape, ZoneSource,
};
use sepika_core::section_shape::SectionShape;
use sepika_design_jp::secondary::eccentricity::{
    center_of_rigidity, story_centers, story_eccentricity,
};
use sepika_design_jp::secondary::eccentricity_analysis::{
    column_horizontal_stiffnesses, column_stiffnesses_from_analysis,
    story_eccentricity_from_analysis,
};
use sepika_design_jp::secondary::story_rigidity::{
    coupling_reason, rigid_floor_story_rigidity, story_rigidity_requiring_coupled_analysis,
};
use sepika_solver::statics::analysis::SeismicDir;
use sepika_solver::statics::linear::{linear_static_once, superpose_static};
use smallvec::smallvec;

#[path = "fixtures/eccentricity_model.rs"]
mod eccentricity_model;
use eccentricity_model::{
    add_case, column_model, element_nodal_residual, oppositely_rotated_columns_rigid_floor,
    three_column_rigid_floor, TOP,
};

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
fn rigid_floor_rigidity_matches_independent_responses_and_holds_coupled_eccentricity() {
    let mut model = three_column_rigid_floor([2400.0, 3200.0]);
    let rigidity = rigid_floor_story_rigidity(&model, TOP).unwrap();
    for (actual, expected) in rigidity
        .center_xy_mm
        .into_iter()
        .zip([2_166.687_027_709_256, 2_862.348_055_979_125])
    {
        assert_relative_eq!(actual, expected, max_relative = 1e-10);
    }
    for (actual, expected) in rigidity.translational_stiffness_n_per_mm.into_iter().zip([
        19_948.412_630_664_1,
        24_529.095_506_958_25,
        34_257.051_676_389_75,
    ]) {
        assert_relative_eq!(actual, expected, max_relative = 1e-10);
    }
    assert_relative_eq!(
        rigidity.torsional_stiffness_nmm,
        31_403_342_934.896_14,
        max_relative = 1e-10
    );

    let x = add_case(
        &mut model,
        "剛心X",
        [1000.0, 0.0, 0.0, 0.0, 0.0, -2_862_348.055_979_125],
    );
    let y = add_case(
        &mut model,
        "剛心Y",
        [0.0, 1000.0, 0.0, 0.0, 0.0, 2_166_687.027_709_256],
    );
    let torsion = add_case(
        &mut model,
        "純ねじり",
        [0.0, 0.0, 0.0, 0.0, 0.0, 1_000_000.0],
    );
    let [res_x, res_y, res_twist] =
        [x, y, torsion].map(|case| linear_static_once(&model, case).unwrap());
    for (result, expected_disp, expected_shear) in [
        (
            &res_x,
            [0.419_316_938_947_756_2, -0.300_243_737_852_773_9, 0.0],
            [1000.0, 0.0],
        ),
        (
            &res_y,
            [-0.300_243_737_852_773_9, 0.244_174_758_533_638_07, 0.0],
            [0.0, 1000.0],
        ),
        (
            &res_twist,
            [
                0.091_147_877_533_713_64,
                -0.068_995_426_130_304_81,
                0.000_031_843_743_580_839_487,
            ],
            [0.0, 0.0],
        ),
    ] {
        let [ux, uy, _, _, _, rz] = result.disp[1];
        for (actual, expected) in [ux, uy, rz].into_iter().zip(expected_disp) {
            assert_relative_eq!(actual, expected, epsilon = 1e-10);
        }
        for (axis, expected) in expected_shear.into_iter().enumerate() {
            assert_relative_eq!(
                result.story_shears.as_ref().unwrap()[axis][0],
                expected,
                epsilon = 1e-8
            );
        }
    }
    let reason = story_eccentricity_from_analysis(&model, TOP, &res_x, &res_y, None).unwrap_err();
    assert!(
        reason.contains("階1") && reason.contains("方向間連成") && reason.contains("弾力半径"),
        "{reason}"
    );
}

#[test]
fn single_plane_inclination_uses_rigid_floor_torsion_without_cross_translation() {
    let mut model = three_column_rigid_floor([2400.0, 0.0]);
    model.sections[0].j = 0.0;
    let rigidity = story_rigidity_requiring_coupled_analysis(&model, TOP, [None; 2])
        .unwrap()
        .expect("片方向傾斜材がある剛床の剛性を取得できません");
    let [kx, kxy, ky] = rigidity.translational_stiffness_n_per_mm;
    assert_relative_eq!(kx, 42_233.858_438_295_02, max_relative = 1e-10);
    assert_relative_eq!(kxy, 0.0, epsilon = 1e-9);
    assert_relative_eq!(ky, 1_734.088_886_550_515, max_relative = 1e-10);
    let [xs, ys] = rigidity.center_xy_mm;
    assert_relative_eq!(xs, 2_188.182_625_259_228, max_relative = 1e-10);
    assert_relative_eq!(ys, 99.368_645_599_998, max_relative = 1e-10);
    assert_relative_eq!(
        rigidity.torsional_stiffness_nmm,
        42_285_751_368.032_24,
        max_relative = 1e-10
    );

    let x = add_case(&mut model, "X", [1000.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    let y = add_case(&mut model, "Y", [0.0, 1000.0, 0.0, 0.0, 0.0, 0.0]);
    let [x, y] = [x, y].map(|case| linear_static_once(&model, case).unwrap());
    let re = story_eccentricity_from_analysis(&model, TOP, &x, &y, None).unwrap();
    assert_relative_eq!(re.kr, 42_285_751_368.032_24, max_relative = 1e-10);
    assert_relative_eq!(re.re_x.unwrap(), 1.899_464_772_228_54, max_relative = 1e-10);
    assert_relative_eq!(
        re.re_y.unwrap(),
        0.038_108_177_062_848,
        max_relative = 1e-10
    );
}

#[test]
fn orthogonal_rigid_floor_matches_arm_sum_when_column_twist_does_no_work() {
    let mut model = three_column_rigid_floor([0.0; 2]);
    let rigidity = rigid_floor_story_rigidity(&model, TOP).unwrap();
    assert_relative_eq!(
        rigidity.torsional_stiffness_nmm,
        33_893_770_491.803_276,
        max_relative = 1e-10
    );
    model.sections[0].j = 0.0;

    let x = add_case(
        &mut model,
        "剛心X",
        [1000.0, 0.0, 0.0, 0.0, 0.0, -2_000_000.0],
    );
    let y = add_case(
        &mut model,
        "剛心Y",
        [0.0, 1000.0, 0.0, 0.0, 0.0, 2_000_000.0],
    );
    let [res_x, res_y] = [x, y].map(|case| linear_static_once(&model, case).unwrap());
    let rigidity = rigid_floor_story_rigidity(&model, TOP).unwrap();
    for (actual, expected) in rigidity.translational_stiffness_n_per_mm.into_iter().zip([
        2_098.360_655_737_705,
        0.0,
        2_098.360_655_737_705,
    ]) {
        assert_relative_eq!(actual, expected, epsilon = 1e-9);
    }
    for center in rigidity.center_xy_mm {
        assert_relative_eq!(center, 2000.0, epsilon = 1e-8);
    }
    assert!(coupling_reason(TOP, rigidity).is_none());
    let ecc = story_eccentricity_from_analysis(&model, TOP, &res_x, &res_y, None).unwrap();
    assert_relative_eq!(ecc.kr, 33_573_770_491.803_276, max_relative = 1e-10);
    assert_relative_eq!(
        rigidity.torsional_stiffness_nmm,
        ecc.kr,
        max_relative = 1e-10
    );
    assert_relative_eq!(ecc.re_x.unwrap(), 0.0, epsilon = 1e-10);
    assert_relative_eq!(ecc.re_y.unwrap(), 0.0, epsilon = 1e-10);
}

#[test]
fn opposite_column_coupling_does_not_cancel_its_moment_about_the_story() {
    let mut model = oppositely_rotated_columns_rigid_floor();
    let rigidity = story_rigidity_requiring_coupled_analysis(&model, TOP, [None; 2])
        .unwrap()
        .expect("柱の連成モーメントを剛床応答で評価");
    let [kx, kxy, ky] = rigidity.translational_stiffness_n_per_mm;
    assert_relative_eq!(kxy, 0.0, epsilon = 1e-9);
    assert_relative_eq!(kx, 2_775.251_189_846_642, max_relative = 1e-10);
    assert_relative_eq!(ky, 2_775.251_189_846_642, max_relative = 1e-10);
    let [xs, ys] = rigidity.center_xy_mm;
    assert_relative_eq!(xs, 2_243.902_439_024_390_3, max_relative = 1e-10);
    assert_relative_eq!(ys, 780.487_804_878_048_9, max_relative = 1e-10);
    assert_relative_eq!(
        rigidity.torsional_stiffness_nmm,
        46_880_447_820.871_66,
        max_relative = 1e-10
    );
    assert!(coupling_reason(TOP, rigidity).is_none());

    let x = add_case(&mut model, "X", [1000.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    let y = add_case(&mut model, "Y", [0.0, 1000.0, 0.0, 0.0, 0.0, 0.0]);
    let [x, y] = [x, y].map(|id| linear_static_once(&model, id).unwrap());
    let eccentricity = story_eccentricity_from_analysis(&model, TOP, &x, &y, None).unwrap();
    assert_relative_eq!(
        eccentricity.kr,
        rigidity.torsional_stiffness_nmm,
        max_relative = 1e-10
    );
    assert_relative_eq!(
        eccentricity.re_x.unwrap(),
        0.296_716_310_799_117_7,
        max_relative = 1e-10
    );
}

#[test]
fn defined_rigidity_does_not_replace_a_missing_column_cut_for_eccentricity() {
    let mut model = oppositely_rotated_columns_rigid_floor();
    model.elements[2].end_offsets_mm = [[0.0, 0.0, 150.0]; 2];
    let rigidity = rigid_floor_story_rigidity(&model, TOP).unwrap();
    assert!(coupling_reason(TOP, rigidity).is_none());
    assert_relative_eq!(
        rigidity.torsional_stiffness_nmm,
        46_880_447_820.871_66,
        max_relative = 1e-10
    );
    let x = add_case(&mut model, "X", [1000.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    let y = add_case(&mut model, "Y", [0.0, 1000.0, 0.0, 0.0, 0.0, 0.0]);
    let [x, y] = [x, y].map(|id| linear_static_once(&model, id).unwrap());
    let reason = story_eccentricity_from_analysis(&model, TOP, &x, &y, None).unwrap_err();
    for expected in ["階1", "柱2", "切断位置"] {
        assert!(reason.contains(expected), "{reason}");
    }
}

#[test]
fn unrelated_components_do_not_hold_an_orthogonal_story() {
    let mut model = three_column_rigid_floor([2400.0, 3200.0]);
    let upper_story = StoryId(2);
    model
        .stories
        .push(Story::new(upper_story, "R".into(), Some(6000.0), Some(0.0)));
    let mut base = model.nodes[2].clone();
    base.id = NodeId(6);
    base.coord[2] = 3000.0;
    base.story = Some(TOP);
    model.nodes.push(base);
    model.stories[TOP.index()].node_ids.push(NodeId(6));
    let mut head = model.nodes[3].clone();
    head.id = NodeId(7);
    head.coord[2] = 6000.0;
    head.story = Some(upper_story);
    model.nodes.push(head);
    model.stories[upper_story.index()].node_ids.push(NodeId(7));
    let mut column = model.elements[1].clone();
    column.id = ElemId(3);
    column.nodes = smallvec![NodeId(6), NodeId(7)];
    model.elements.push(column);
    let lower_reason =
        story_rigidity_requiring_coupled_analysis(&model, TOP, [None; 2]).unwrap_err();
    assert!(
        lower_reason.contains("階1")
            && lower_reason.contains("部材0")
            && lower_reason.contains("全1層"),
        "{lower_reason}"
    );
    assert_eq!(
        story_rigidity_requiring_coupled_analysis(&model, upper_story, [None; 2]).unwrap(),
        None
    );
}

#[test]
fn inclined_plane_requires_evaluation_when_its_perpendicular_plane_is_connected() {
    let mut model = three_column_rigid_floor([1000.0, 0.0]);
    model.constraints.clear();

    let mut end = model.nodes[3].clone();
    end.id = NodeId(6);
    end.coord[1] = 6000.0;
    model.nodes.push(end);
    model.stories[TOP.index()].node_ids.push(NodeId(6));
    let mut section = model.sections[0].clone();
    section.id = SectionId(1);
    section.name = "梁断面".into();
    section.frame_use = Some(FrameSectionUse::Girder);
    model.sections.push(section);
    let mut transverse = model.elements[0].clone();
    transverse.id = ElemId(3);
    transverse.nodes = smallvec![NodeId(3), NodeId(6)];
    transverse.section = Some(SectionId(1));
    model.elements.push(transverse);

    assert_eq!(
        story_rigidity_requiring_coupled_analysis(&model, TOP, [None; 2]).unwrap(),
        None
    );
    model.elements[3].nodes = smallvec![NodeId(1), NodeId(5)];
    let reason = story_rigidity_requiring_coupled_analysis(&model, TOP, [None; 2]).unwrap_err();
    for expected in ["階1", "傾斜部材0", "直交側の構面材3", "単一剛床"] {
        assert!(reason.contains(expected), "{reason}");
    }
}

#[test]
fn orthogonal_members_need_an_evaluation_when_free_out_of_plane_dofs_connect_them() {
    let mut model = three_column_rigid_floor([0.0; 2]);
    model.constraints.clear();
    let mut section = model.sections[0].clone();
    section.id = SectionId(1);
    section.name = "梁断面".into();
    section.frame_use = Some(FrameSectionUse::Girder);
    model.sections.push(section);
    for (end, id) in [(NodeId(3), ElemId(3)), (NodeId(5), ElemId(4))] {
        let mut beam = model.elements[0].clone();
        beam.id = id;
        beam.nodes = smallvec![NodeId(1), end];
        beam.section = Some(SectionId(1));
        beam.local_axis.ref_vector = [0.0, 0.0, 1.0];
        model.elements.push(beam);
    }
    for head in model.nodes.iter_mut().skip(1).step_by(2) {
        for dof in [Dof::Uz, Dof::Rx, Dof::Ry] {
            head.restraint.set_free(dof);
        }
        head.restraint.set_fixed(Dof::Rz);
    }
    let x = add_case(&mut model, "X", [1000.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    let x = linear_static_once(&model, x).unwrap();
    assert!(x.disp[1][1].abs() > 1e-8, "{:?}", x.disp[1]);
    let reason = story_rigidity_requiring_coupled_analysis(&model, TOP, [None; 2]).unwrap_err();
    assert!(
        reason.contains("直交構面材3・4") && reason.contains("自由なUz"),
        "{reason}"
    );

    for head in model.nodes.iter_mut().skip(1).step_by(2) {
        head.restraint.set_fixed(Dof::Uz);
        head.restraint.set_free(Dof::Rz);
    }
    let reason = story_rigidity_requiring_coupled_analysis(&model, TOP, [None; 2]).unwrap_err();
    assert!(
        reason.contains("直交構面材3・4") && reason.contains("自由なRz"),
        "{reason}"
    );
    for head in model.nodes.iter_mut().skip(1).step_by(2) {
        head.restraint.set_fixed(Dof::Rz);
    }
    assert_eq!(
        story_rigidity_requiring_coupled_analysis(&model, TOP, [None; 2]).unwrap(),
        None
    );
}

#[test]
fn transverse_brace_offsets_can_couple_orthogonal_axes_through_nodal_rotation() {
    let mut model = three_column_rigid_floor([0.0; 2]);
    model.constraints.clear();
    model.girder_torsion = sepika_core::model::GirderTorsionMode::Keep;
    let mut section = model.sections[0].clone();
    section.id = SectionId(1);
    section.name = "斜材断面".into();
    section.frame_use = Some(FrameSectionUse::Brace);
    model.sections.push(section);
    for (end, id) in [(NodeId(3), ElemId(3)), (NodeId(5), ElemId(4))] {
        let mut brace = model.elements[0].clone();
        brace.id = id;
        brace.nodes = smallvec![NodeId(1), end];
        brace.section = Some(SectionId(1));
        brace.kind = ElementKind::Brace {
            tension_only: false,
        };
        model.elements.push(brace);
    }
    let case = add_case(&mut model, "X", [1000.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    let independent = linear_static_once(&model, case).unwrap();
    assert_relative_eq!(independent.disp[1][1], 0.0, epsilon = 1e-12);
    assert_eq!(
        story_rigidity_requiring_coupled_analysis(&model, TOP, [None; 2]).unwrap(),
        None
    );
    model.elements[3].end_offsets_mm = [[0.0, 350.0, 0.0]; 2];
    model.elements[4].end_offsets_mm = [[450.0, 0.0, 0.0]; 2];
    let coupled = linear_static_once(&model, case).unwrap();
    assert!(coupled.disp[1][1].abs() > 1e-8, "{:?}", coupled.disp[1]);
    let reason = story_rigidity_requiring_coupled_analysis(&model, TOP, [None; 2]).unwrap_err();
    for expected in ["階1", "部材3", "端部偏心", "単一剛床"] {
        assert!(reason.contains(expected), "{reason}");
    }
}

#[test]
fn orthogonal_offsets_on_segments_can_couple_through_the_shared_vertical_dof() {
    let mut model = three_column_rigid_floor([0.0; 2]);
    model.constraints.clear();
    for head in model.nodes.iter_mut().skip(1).step_by(2) {
        head.restraint.set_fixed(Dof::Rz);
    }
    let mut joint = model.nodes[1].clone();
    joint.id = NodeId(6);
    joint.coord[2] = 1500.0;
    joint.restraint = Dof6Mask::FREE;
    joint.restraint.set_fixed(Dof::Rz);
    model.nodes.push(joint);
    let mut upper = model.elements[0].clone();
    upper.id = ElemId(3);
    upper.nodes[0] = NodeId(6);
    model.elements[0].nodes[1] = NodeId(6);
    model.elements[0].end_offsets_mm = [[100.0, 0.0, 0.0]; 2];
    model.elements.push(upper);
    let case = add_case(&mut model, "X", [1000.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    let independent = linear_static_once(&model, case).unwrap();
    assert_relative_eq!(independent.disp[1][1], 0.0, epsilon = 1e-12);
    assert_eq!(
        story_rigidity_requiring_coupled_analysis(&model, TOP, [None; 2]).unwrap(),
        None
    );
    model.elements[3].end_offsets_mm = [[0.0, 200.0, 0.0]; 2];
    let coupled = linear_static_once(&model, case).unwrap();
    assert!(coupled.disp[1][1].abs() > 1e-8, "{:?}", coupled.disp[1]);
    let reason = story_rigidity_requiring_coupled_analysis(&model, TOP, [None; 2]).unwrap_err();
    for expected in ["階1", "構面材0・3", "節点6", "自由なUz"] {
        assert!(reason.contains(expected), "{reason}");
    }
}

#[test]
fn pinned_inclined_brace_and_offset_column_can_couple_through_vertical_displacement() {
    let mut model = three_column_rigid_floor([0.0; 2]);
    model.constraints.clear();
    for head in model.nodes.iter_mut().skip(1).step_by(2) {
        head.restraint.set_fixed(Dof::Rz);
    }
    model.nodes[1].restraint.set_free(Dof::Uz);
    model.nodes[1].restraint.set_free(Dof::Rx);
    let mut anchor = model.nodes[0].clone();
    anchor.id = NodeId(6);
    anchor.coord[0] = -2000.0;
    model.nodes.push(anchor);
    let mut section = model.sections[0].clone();
    section.id = SectionId(1);
    section.name = "斜材断面".into();
    section.frame_use = Some(FrameSectionUse::Brace);
    model.sections.push(section);
    let mut brace = model.elements[0].clone();
    brace.id = ElemId(3);
    brace.nodes[0] = NodeId(6);
    brace.section = Some(SectionId(1));
    brace.kind = ElementKind::Brace {
        tension_only: false,
    };
    brace.end_cond = [EndCondition::Pinned; 2];
    model.elements.push(brace);

    let case = add_case(&mut model, "X", [1000.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    let independent = linear_static_once(&model, case).unwrap();
    assert_relative_eq!(independent.disp[1][1], 0.0, epsilon = 1e-12);
    assert_eq!(
        story_rigidity_requiring_coupled_analysis(&model, TOP, [None; 2]).unwrap(),
        None
    );

    model.elements[0].end_offsets_mm = [[0.0, 250.0, 0.0]; 2];
    let coupled = linear_static_once(&model, case).unwrap();
    assert_relative_eq!(
        coupled.disp[1][1].abs(),
        0.866_828_388_516_302,
        epsilon = 1e-10
    );
    let reason = story_rigidity_requiring_coupled_analysis(&model, TOP, [None; 2]).unwrap_err();
    for expected in ["階1", "構面材3・0", "節点1", "自由なUz", "単一剛床"] {
        assert!(reason.contains(expected), "{reason}");
    }
}

#[test]
fn orthogonal_walls_on_free_vertical_joints_also_require_an_evaluation() {
    let mut model = three_column_rigid_floor([0.0; 2]);
    model.constraints.clear();
    for head in model.nodes.iter_mut().skip(1).step_by(2) {
        head.restraint.set_free(Dof::Uz);
    }
    let mut section = model.sections[0].clone();
    section.id = SectionId(1);
    section.name = "壁断面".into();
    section.frame_use = None;
    section.thickness = Some(150.0);
    model.sections.push(section);
    for (id, nodes) in [
        (ElemId(3), [NodeId(0), NodeId(2), NodeId(3), NodeId(1)]),
        (ElemId(4), [NodeId(0), NodeId(4), NodeId(5), NodeId(1)]),
    ] {
        let mut wall = model.elements[0].clone();
        wall.id = id;
        wall.kind = ElementKind::Wall;
        wall.nodes = nodes.into_iter().collect();
        wall.section = Some(SectionId(1));
        model.elements.push(wall);
    }
    let reason = story_rigidity_requiring_coupled_analysis(&model, TOP, [None; 2]).unwrap_err();
    assert!(
        reason.contains("直交構面材3・4") && reason.contains("自由なUz"),
        "{reason}"
    );
}

#[test]
fn nodal_spring_candidate_depends_on_active_global_cross_stiffness() {
    let mut model = three_column_rigid_floor([0.0; 2]);
    model.constraints.clear();
    model.nodes[1].restraint.set_fixed(Dof::Rz);
    let mut anchor = model.nodes[0].clone();
    anchor.id = NodeId(6);
    anchor.coord = [-1000.0, -1000.0, 3000.0];
    anchor.restraint = Dof6Mask::FIXED;
    model.nodes.push(anchor);
    let mut spring = model.elements[0].clone();
    spring.id = ElemId(3);
    spring.kind = ElementKind::NodalSpring;
    spring.nodes = smallvec![NodeId(6), NodeId(1)];
    spring.section = None;
    spring.local_axis.ref_vector = [-1.0, 1.0, 0.0];
    model.elements.push(spring);
    for stiffness in [[0.0; 6], [1000.0, 1000.0, 0.0, 0.0, 0.0, 0.0]] {
        model.elements[3].spring = Some(stiffness);
        assert_eq!(
            story_rigidity_requiring_coupled_analysis(&model, TOP, [None; 2]).unwrap(),
            None
        );
    }
    model.elements[3].spring = Some([2000.0, 1000.0, 0.0, 0.0, 0.0, 0.0]);
    let reason = story_rigidity_requiring_coupled_analysis(&model, TOP, [None; 2]).unwrap_err();
    for expected in ["階1", "部材3", "付加ばね", "単一剛床"] {
        assert!(reason.contains(expected), "{reason}");
    }

    model.elements[3].spring = Some([f64::NAN, 1000.0, 0.0, 0.0, 0.0, 0.0]);
    let reason = story_rigidity_requiring_coupled_analysis(&model, TOP, [None; 2]).unwrap_err();
    assert!(
        reason.contains("階1") && reason.contains("節点ばね3の有効な方向別剛性"),
        "{reason}"
    );
}

#[test]
fn rotated_column_with_an_isotropic_section_needs_evaluation_when_a_wing_wall_changes_stiffness() {
    let mut model = three_column_rigid_floor([0.0; 2]);
    model.constraints.clear();
    model.elements[0].local_axis.ref_vector = [1.0, 1.0, 0.0];
    model.materials[0].category = MaterialCategory::Concrete;
    model.materials[0].fc = Some(24.0);
    model.sections[0].width = 300.0;
    model.sections[0].depth = 300.0;
    assert_eq!(
        story_rigidity_requiring_coupled_analysis(&model, TOP, [None; 2]).unwrap(),
        None
    );

    let mut wall_section = SectionShape::RcWall {
        thickness: 150.0,
        ps: 0.0025,
        pwh_ratio: None,
        pwv_ratio: None,
    }
    .to_section(SectionId(1), "袖壁断面".into());
    wall_section.material = Some(model.materials[0].id);
    model.sections.push(wall_section);
    let boundary = [NodeId(0), NodeId(2), NodeId(3), NodeId(1)];
    let mut wall = model.elements[0].clone();
    wall.id = ElemId(3);
    wall.kind = ElementKind::Wall;
    wall.section = Some(SectionId(1));
    wall.nodes = boundary.into_iter().collect();
    model.elements.push(wall);
    let openings = vec![WallOpening {
        width: 4800.0,
        height: 2500.0,
        offset: Some([900.0, 250.0]),
    }];
    model.wall_attrs.push(WallAttr {
        elem: ElemId(3),
        opening_area: 0.0,
        opening_weight: 0.0,
        slit: Default::default(),
        openings: openings.clone(),
        finish_intensity: 0.0,
    });
    model.add_enclosed_wall_plate_from_nodes(
        &boundary,
        WallPlate {
            dl_support: None,
            self_weight_shares: Vec::new(),
            id: sepika_core::ids::WallPlateId(0),
            shape: WallPlateShape::Enclosed,
            section: Some(SectionId(1)),
            opening_area: 0.0,
            opening_weight: 0.0,
            openings,
            loads: Vec::new(),
            slit: Default::default(),
        },
    );
    let beam =
        sepika_element::frame::beam::BeamElement::try_new(&model.elements[0], &model).unwrap();
    assert_ne!(beam.iy, beam.iz);
    assert_ne!(beam.as_y, beam.as_z);
    let reason = story_rigidity_requiring_coupled_analysis(&model, TOP, [None; 2]).unwrap_err();
    assert!(
        reason.contains("階1") && reason.contains("部材0の方向別断面剛性"),
        "{reason}"
    );

    model.sections[0].material = None;
    let reason = story_rigidity_requiring_coupled_analysis(&model, TOP, [None; 2]).unwrap_err();
    assert!(
        reason.contains("階1") && reason.contains("部材0の実効断面剛性"),
        "{reason}"
    );
}

#[test]
fn floor_rigidity_uses_the_existing_sl_tolerance_for_diaphragm_and_support_nodes() {
    let mut model = three_column_rigid_floor([2400.0, 3200.0]);
    for (index, node) in model.nodes.iter_mut().enumerate() {
        node.coord[2] += if index % 2 == 0 { -0.5 } else { 0.5 };
    }
    let rigidity = rigid_floor_story_rigidity(&model, TOP).unwrap();
    assert!(rigidity.torsional_stiffness_nmm > 0.0);
    assert!(coupling_reason(TOP, rigidity).is_some());

    let mut outside_base = model.clone();
    outside_base.nodes[0].coord[2] = -1.25;
    model.nodes[5].coord[2] = 3001.25;
    for (outside, expected) in [
        (model, "節点5が上端SLの剛床判定範囲外です"),
        (outside_base, "節点0の高さ又は支持条件が対象外です"),
    ] {
        let reason = rigid_floor_story_rigidity(&outside, TOP).unwrap_err();
        assert!(
            reason.contains("階1") && reason.contains(expected),
            "{reason}"
        );
    }
}

#[test]
fn malformed_detached_node_holds_both_d_value_entries_with_the_layer() {
    let mut model = three_column_rigid_floor([0.0; 2]);
    let mut detached = model.nodes[0].clone();
    detached.id = NodeId(99);
    detached.story = None;
    model.nodes.push(detached);
    for reason in [
        story_centers(&model, TOP).unwrap_err(),
        story_eccentricity(&model, TOP).unwrap_err(),
    ] {
        assert!(
            reason.contains("階1") && reason.contains("節点ID 99"),
            "{reason}"
        );
    }
}

#[test]
fn d_value_entry_points_hold_when_an_unexpanded_wall_plane_is_diagonal() {
    let mut model = three_column_rigid_floor([0.0; 2]);
    model.nodes[2].coord[1] = 4000.0;
    model.nodes[3].coord[1] = 4000.0;
    model.materials[0].category = MaterialCategory::Concrete;
    model.materials[0].fc = Some(24.0);
    let mut section = SectionShape::RcWall {
        thickness: 150.0,
        ps: 0.0025,
        pwh_ratio: None,
        pwv_ratio: None,
    }
    .to_section(SectionId(1), "斜め壁断面".into());
    section.material = Some(model.materials[0].id);
    model.sections.push(section);
    let boundary = [NodeId(0), NodeId(2), NodeId(3), NodeId(1)];
    let plate_id = model.add_enclosed_wall_plate_from_nodes(
        &boundary,
        WallPlate {
            dl_support: None,
            self_weight_shares: Vec::new(),
            id: sepika_core::ids::WallPlateId(0),
            shape: WallPlateShape::Enclosed,
            section: Some(SectionId(1)),
            opening_area: 0.0,
            opening_weight: 0.0,
            openings: Vec::new(),
            loads: Vec::new(),
            slit: Default::default(),
        },
    );
    model.wall_regions.push(sepika_core::model::WallRegion {
        id: sepika_core::ids::WallRegionId(0),
        name: String::new(),
        boundary: boundary.to_vec(),
        wall_plate_ids: vec![plate_id],
        posts: Vec::new(),
    });
    assert!(model
        .elements
        .iter()
        .all(|element| element.kind != ElementKind::Wall));
    for reason in [
        story_centers(&model, TOP).unwrap_err(),
        story_eccentricity(&model, TOP).unwrap_err(),
    ] {
        for expected in ["階1", "面材", "X/Y両方向", "D値略算法"] {
            assert!(reason.contains(expected), "{reason}");
        }
    }
}

#[test]
fn malformed_node_ids_hold_rigidity_and_eccentricity_with_the_layer() {
    let mut model = three_column_rigid_floor([2400.0, 3200.0]);
    let x = add_case(&mut model, "X", [1000.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    let y = add_case(&mut model, "Y", [0.0, 1000.0, 0.0, 0.0, 0.0, 0.0]);
    let [x, y] = [x, y].map(|id| linear_static_once(&model, id).unwrap());
    let mut out_of_range = model.clone();
    out_of_range.nodes[1].id = NodeId(99);
    let mut wrong_index = model.clone();
    wrong_index.nodes.swap(0, 1);
    let mut missing_element_node = model.clone();
    missing_element_node.elements[0].nodes[1] = NodeId(99);
    let mut missing_master = model;
    let Constraint::RigidDiaphragm { master, .. } = &mut missing_master.constraints[0] else {
        panic!("試験モデルは層上端に剛床を持ちます");
    };
    *master = NodeId(99);
    for (invalid, expected) in [
        (out_of_range, "節点ID 99"),
        (wrong_index, "節点ID 1"),
        (missing_element_node, "部材0が参照する節点ID 99"),
        (missing_master, "節点拘束1が参照する節点ID 99"),
    ] {
        for reason in [
            rigid_floor_story_rigidity(&invalid, TOP).unwrap_err(),
            story_eccentricity_from_analysis(&invalid, TOP, &x, &y, None).unwrap_err(),
        ] {
            assert!(
                reason.contains("階1") && reason.contains(expected),
                "{reason}"
            );
        }
    }
}

#[test]
fn stored_story_rigidity_requires_consistent_finite_positive_identification() {
    let mut model = three_column_rigid_floor([2400.0, 3200.0]);
    let x = add_case(&mut model, "X", [1000.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    let y = add_case(&mut model, "Y", [0.0, 1000.0, 0.0, 0.0, 0.0, 0.0]);
    let value = rigid_floor_story_rigidity(&model, TOP).unwrap();
    let [mut x, y] = [x, y].map(|id| linear_static_once(&model, id).unwrap());
    assert!(story_eccentricity_from_analysis(&model, TOP, &x, &y, None)
        .unwrap_err()
        .contains("Kxy"));
    x.story_rigidity = vec![(TOP, Ok(Some(value)))];
    let hold_reason = |other: &sepika_solver::statics::linear::StaticOnce| {
        story_eccentricity_from_analysis(&model, TOP, &x, other, None).unwrap_err()
    };
    let assert_reason = |reason: String, expected| {
        assert!(
            reason.contains("階1") && reason.contains(expected),
            "{reason}"
        );
    };
    assert_reason(hold_reason(&y), "X/Y方向の層剛性記録が一致しません");

    let mut y = y;
    y.story_rigidity = vec![(TOP, Ok(Some(value)))];
    assert_reason(hold_reason(&y), "Kxy");
    y.story_rigidity.push((TOP, Ok(Some(value))));
    assert_reason(hold_reason(&y), "Y方向の層剛性記録が重複");
    y.story_rigidity = vec![(TOP, Ok(None))];
    assert_reason(hold_reason(&y), "X/Y方向の層剛性記録が一致しません");
    let mut zero_cross = value;
    zero_cross.translational_stiffness_n_per_mm[1] = 0.0;
    y.story_rigidity = vec![(TOP, Ok(Some(zero_cross)))];
    assert_reason(hold_reason(&y), "X/Y方向の層剛性記録が一致しません");

    let mut invalid_center = value;
    invalid_center.center_xy_mm[0] = f64::NAN;
    let mut invalid_torsion = value;
    invalid_torsion.torsional_stiffness_nmm = 0.0;
    let mut indefinite = value;
    indefinite.translational_stiffness_n_per_mm = [1.0, 2.0, 1.0];
    for invalid in [invalid_center, invalid_torsion, indefinite] {
        y.story_rigidity = vec![(TOP, Ok(Some(invalid)))];
        assert_reason(hold_reason(&y), "Y方向の層剛性記録に非有限");
    }
    y.story_rigidity = vec![(TOP, Ok(Some(value)))];
    model.nodes[1].id = NodeId(99);
    assert_reason(
        story_eccentricity_from_analysis(&model, TOP, &x, &y, None).unwrap_err(),
        "節点ID 99",
    );
}

#[test]
fn stored_and_unrecorded_rigidity_reject_stories_without_a_layer() {
    let mut model = three_column_rigid_floor([0.0; 2]);
    let case = add_case(&mut model, "X", [1000.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    let mut result = linear_static_once(&model, case).unwrap();
    let value = rigid_floor_story_rigidity(&model, TOP).unwrap();
    for story in [StoryId(0), StoryId(99)] {
        result.story_rigidity = vec![(story, Ok(Some(value)))];
        for responses in [[None; 2], [Some(&result); 2]] {
            let reason =
                story_rigidity_requiring_coupled_analysis(&model, story, responses).unwrap_err();
            assert!(reason.contains(&format!("階{}", story.0)), "{reason}");
            assert!(
                reason.contains("上端階に対応する層がありません"),
                "{reason}"
            );
        }
    }
}

#[test]
fn coupled_rigidity_reports_the_layer_and_failed_identification_condition() {
    let model = three_column_rigid_floor([2400.0, 3200.0]);
    let mut no_floor = model.clone();
    no_floor.constraints.clear();
    let mut many_layers = model.clone();
    many_layers
        .stories
        .push(Story::new(StoryId(2), "R".into(), Some(6000.0), Some(0.0)));
    let mut with_spring = model.clone();
    with_spring.nodes[1].support_spring = Some([1.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    let mut multiple_floors = model.clone();
    multiple_floors
        .constraints
        .push(Constraint::rigid_diaphragm(TOP, NodeId(3), Vec::new()));
    let mut active_set = model.clone();
    active_set.stress_cfg.tension_only_iteration = true;
    let mut section = active_set.sections[0].clone();
    section.id = SectionId(2);
    section.name = "ブレース断面".into();
    section.frame_use = Some(FrameSectionUse::Brace);
    active_set.sections.push(section);
    let mut brace = active_set.elements[1].clone();
    brace.id = ElemId(3);
    brace.kind = ElementKind::Brace { tension_only: true };
    brace.section = Some(SectionId(2));
    active_set.elements.push(brace);
    let mut unrestrained_base = model;
    unrestrained_base.nodes[0].restraint.set_free(Dof::Rz);
    let mut invalid_wall = unrestrained_base.clone();
    invalid_wall.stress_cfg.misc_wall_n = Some(2.0);
    invalid_wall.wall_plates.push(WallPlate {
        dl_support: None,
        self_weight_shares: Vec::new(),
        id: sepika_core::ids::WallPlateId(0),
        shape: WallPlateShape::Attached {
            anchor: sepika_core::model::RegionAnchor::FloorRegion {
                nodes: [NodeId(0), NodeId(2)],
            },
            extent: None,
        },
        section: None,
        opening_area: f64::NAN,
        opening_weight: 0.0,
        openings: Vec::new(),
        loads: Vec::new(),
        slit: Default::default(),
    });
    for (changed, expected) in [
        (no_floor, "単一剛床"),
        (many_layers, "全1層"),
        (with_spring, "支持ばね"),
        (multiple_floors, "剛床が複数"),
        (active_set, "活動状態が加力によって変わります"),
        (unrestrained_base, "面内移動・回転が固定されていません"),
        (invalid_wall, "n倍法: 壁0: 開口面積が不正です"),
    ] {
        let reason =
            story_rigidity_requiring_coupled_analysis(&changed, TOP, [None; 2]).unwrap_err();
        assert!(
            reason.contains("階1") && reason.contains(expected),
            "{reason}"
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
