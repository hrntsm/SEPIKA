#[path = "fixtures/rc_beam_asymmetric_model.rs"]
mod beam;
#[path = "fixtures/rc_column_axial_boundary_model.rs"]
mod column;

use sepika_core::ids::{ElemId, NodeId, SectionId};
use sepika_core::model::{
    default_combinations, default_load_cases, Haunch, MemberDetailAttr, Model,
};
use sepika_core::rc_capacity::RcUltimateShearActions;
use sepika_core::section_shape::{RebarSize, SectionShape, StandardRebar};
use sepika_design_jp::ultimate::{
    collect_rc_ultimate_checks, MemberDemand, UltimateCheck, UltimateShearOptions,
};

fn model(is_beam: bool) -> Model {
    let mut model = if is_beam {
        beam::model(false)
    } else {
        column::column_model(false, 0.0)
    };
    model.load_cases = default_load_cases();
    model.combinations = default_combinations();
    model.ultimate_gravity_combination = Some(0);
    model.elements[0].rigid_zone.face_i = Some(100.0);
    model.elements[0].rigid_zone.face_j = Some(150.0);
    let main = RebarSize::Standard(StandardRebar::D19);
    let shear = RebarSize::Standard(StandardRebar::D10);
    match model.sections[0].shape.as_mut().unwrap() {
        SectionShape::RcBeamRect { rebar, .. } => {
            rebar.main_dia = main;
            rebar.stirrup.dia = shear;
        }
        SectionShape::RcColumnRect { rebar, .. } => {
            rebar.main_dia = main;
            rebar.hoop.dia = shear;
        }
        _ => unreachable!(),
    }
    model.materials[1].strength_factor = Some(1.1);
    model
}

fn demand(axial_range_n: [f64; 2], max_mq: [(f64, f64); 2]) -> MemberDemand {
    MemberDemand {
        rc_ultimate_shear: Some(RcUltimateShearActions {
            axial_range_n,
            max_mq,
        }),
        ..MemberDemand::from_pushover(axial_range_n[1], 5.0e8, 0.0, 2.0e5, 0.0, 0.0)
    }
}

fn check(model: &Model, demand: MemberDemand, weak: bool) -> UltimateCheck {
    let options = UltimateShearOptions {
        include_bond: false,
        biaxial_shear: weak,
        ..Default::default()
    };
    collect_rc_ultimate_checks(model, &[(ElemId(0), demand)], &options)
        .unwrap()
        .remove(0)
}

#[test]
fn beam_and_compressed_column_use_the_confirmed_face_actions() {
    for (beam, axial, moment_nmm, expected_capacity_n, expected_margin) in [
        (
            true,
            [0.0, 0.0],
            58_100_000.0,
            164_779.006_020,
            1.647_790_060_20,
        ),
        (
            false,
            [480_000.0, 600_000.0],
            108_100_000.0,
            422_333.711_731,
            4.223_337_117_31,
        ),
    ] {
        let result = check(
            &model(beam),
            demand(axial, [(moment_nmm, 100_000.0), (0.0, 0.0)]),
            false,
        );
        assert!(result.ok, "{result:?}");
        assert_eq!(result.qmu, 100_000.0);
        assert!(
            (result.qsu - expected_capacity_n).abs() < 1e-5,
            "{result:?}"
        );
        assert!(
            (result.shear_margin - expected_margin).abs() < 1e-10,
            "{result:?}"
        );
    }
}

#[test]
fn column_caps_both_shear_reinforcement_ratio_and_axial_stress() {
    let mut model = model(false);
    let SectionShape::RcColumnRect { rebar, .. } = model.sections[0].shape.as_mut().unwrap() else {
        unreachable!()
    };
    rebar.hoop.legs_x = 8;
    rebar.hoop.pitch = 40.0;
    let result = check(
        &model,
        demand([5_000_000.0; 2], [(108_100_000.0, 100_000.0), (0.0, 0.0)]),
        false,
    );
    assert!(result.ok, "{result:?}");
    assert!((result.qsu - 714_917.250_471).abs() < 1e-5, "{result:?}");
    assert!(
        (result.shear_margin - 7.149_172_504_71).abs() < 1e-10,
        "{result:?}"
    );
}

#[test]
fn weak_column_axis_has_its_own_capacity_and_does_not_imply_biaxial_safety() {
    let model = model(false);
    let weak = (68_100_000.0, 100_000.0);
    let axial = [480_000.0; 2];
    let uniaxial = check(&model, demand(axial, [(0.0, 0.0), weak]), true);
    assert!(uniaxial.ok && uniaxial.qsu.is_nan(), "{uniaxial:?}");
    let (demand_n, capacity_n) = uniaxial.weak_shear_n.unwrap();
    assert_eq!(demand_n, 100_000.0);
    assert!((capacity_n - 370_711.430_450).abs() < 1e-5);
    assert!(
        (uniaxial.weak_shear_margin.unwrap() - 3.707_114_304_50).abs() < 1e-10,
        "{uniaxial:?}"
    );
    let both = demand(axial, [(108_100_000.0, 100_000.0), weak]);
    for enabled in [false, true] {
        let result = check(&model, both, enabled);
        assert!(!result.ok && result.shear_margin > 1.0, "{result:?}");
        assert_eq!(result.weak_shear_margin.is_some(), enabled);
        assert!(result.unchecked_reason.unwrap().contains("二軸同時作用"));
    }
}

#[test]
fn unverified_actions_and_inapplicable_members_do_not_produce_capacity_or_margin() {
    let column = model(false);
    let mut light = column.clone();
    light.materials[0].concrete_class = sepika_core::units::ConcreteClass::Lightweight1;
    let mut fy_low = column.clone();
    fy_low.materials[1].fy = Some(300.0);
    let mut multilayer = column.clone();
    let SectionShape::RcColumnRect { rebar, .. } = multilayer.sections[0].shape.as_mut().unwrap()
    else {
        unreachable!()
    };
    rebar.x.push(2);
    rebar.y[0] = 4;
    let mut haunch = model(true);
    haunch.member_detail_attrs.push(MemberDetailAttr {
        haunch_i: Some(Haunch {
            length: 600.0,
            depth_increase: 100.0,
            width_increase: 0.0,
            qd1_face: None,
        }),
        ..MemberDetailAttr::new(ElemId(0))
    });
    let mq = [(108_100_000.0, 100_000.0), (0.0, 0.0)];
    let verified = demand([0.0; 2], mq);
    let unverified = MemberDemand {
        rc_ultimate_shear: None,
        ..verified
    };
    for (model, demand, reason) in [
        (&column, unverified, "断面力の範囲"),
        (&column, demand([-1.0, 0.0], mq), "柱は非引張"),
        (&light, verified, "普通コンクリート"),
        (&fy_low, verified, "材種の基準強度を下回っています"),
        (&multilayer, verified, "多段配筋"),
        (&haunch, verified, "ハンチ付き"),
    ] {
        let result = check(model, demand, false);
        assert!(
            !result.ok && result.qsu.is_nan() && result.shear_margin.is_nan(),
            "{result:?}"
        );
        assert!(
            result.unchecked_reason.as_deref().unwrap().contains(reason),
            "{result:?}"
        );
    }
}

#[test]
fn direct_confirmed_demand_distinguishes_missing_shear_from_zero_shear() {
    let model = model(true);
    let verified = MemberDemand {
        min_axial_for_shear_n: Some(0.0),
        ..MemberDemand::from_pushover(0.0, 58_100_000.0, 0.0, 100_000.0, 0.0, 0.0)
    };
    assert!(check(&model, verified, false).ok);
    for input in [
        MemberDemand {
            shear: None,
            ..verified
        },
        MemberDemand {
            shear_weak: None,
            ..verified
        },
    ] {
        let error = collect_rc_ultimate_checks(&model, &[(ElemId(0), input)], &Default::default())
            .unwrap_err();
        assert!(error.contains("ultimate_total_unsupported"), "{error}");
    }
}

#[test]
fn grouped_beam_never_uses_one_segments_maxima_as_whole_member_actions() {
    let mut model = model(true);
    let mut end = model.nodes[1].clone();
    end.id = NodeId(2);
    end.coord[0] = 6_000.0;
    model.nodes.push(end);
    let mut segment = model.elements[0].clone();
    segment.id = ElemId(1);
    segment.nodes[0] = NodeId(1);
    segment.nodes[1] = NodeId(2);
    model.elements.push(segment);
    let demands = [
        (
            ElemId(0),
            demand([0.0; 2], [(100_000_000.0, 50_000.0), (0.0, 0.0)]),
        ),
        (
            ElemId(1),
            demand([0.0; 2], [(50_000_000.0, 100_000.0), (0.0, 0.0)]),
        ),
    ];
    let opts = UltimateShearOptions {
        include_bond: false,
        ..Default::default()
    };
    let separate = collect_rc_ultimate_checks(&model, &demands, &opts).unwrap();
    assert!(
        separate
            .iter()
            .all(|check| check.ok && check.qsu.is_finite()),
        "{separate:?}"
    );
    model.girder_groups = vec![vec![ElemId(0), ElemId(1)]];
    let grouped = collect_rc_ultimate_checks(&model, &demands, &opts).unwrap();
    assert_eq!(grouped.len(), demands.len());
    for result in grouped {
        assert!(
            !result.ok && result.qsu.is_nan() && result.shear_margin.is_nan(),
            "{result:?}"
        );
        assert!(result
            .unchecked_reason
            .unwrap()
            .contains("一本部材グループ全体"));
    }
}

#[test]
fn missing_whole_beam_record_is_reported_even_when_the_outer_face_passes_the_first_interval() {
    use sepika_core::{adjacency::NodeAdjacency, face_distance::girder_column_faces};

    let mut model = model(true);
    let mut end = model.nodes[1].clone();
    end.id = NodeId(2);
    model.nodes[1].coord[0] = 100.0;
    model.nodes.push(end);
    let mut right = model.elements[0].clone();
    right.id = ElemId(1);
    right.nodes = [NodeId(1), NodeId(2)].into_iter().collect();
    right.rigid_zone.face_i = Some(0.0);
    model.elements.push(right);

    let mut support = self::model(false);
    let mut section = support.sections.remove(0);
    section.id = SectionId(1);
    model.sections.push(section);
    let mut base = support.nodes.remove(0);
    base.id = NodeId(3);
    base.coord[2] = -3_000.0;
    model.nodes.push(base);
    let mut column = support.elements.remove(0);
    column.id = ElemId(2);
    column.section = Some(SectionId(1));
    column.nodes = [NodeId(3), NodeId(0)].into_iter().collect();
    column.local_axis.ref_vector = [0.0, 1.0, 0.0];
    model.elements.push(column);

    let mut whole_beam = model.elements[0].clone();
    whole_beam.nodes[1] = NodeId(2);
    let [face_i, _] =
        girder_column_faces(&model, &whole_beam, &NodeAdjacency::primary_frame(&model)).unwrap();
    let face_mm = face_i.unwrap().position_mm;
    assert_eq!(face_mm, 200.0);
    model.elements[0].rigid_zone.face_i = Some(face_mm);
    model.elements[0].rigid_zone.face_j = Some(0.0);
    model.girder_groups = vec![vec![ElemId(0), ElemId(1)]];

    let demands = [ElemId(0), ElemId(1), ElemId(2)].map(|id| {
        (
            id,
            MemberDemand::from_pushover(0.0, 80e6, 0.0, 60e3, 0.0, 0.0),
        )
    });
    let opts = UltimateShearOptions {
        include_bond: false,
        ..Default::default()
    };
    let checks = collect_rc_ultimate_checks(&model, &demands, &opts).unwrap();
    for id in [ElemId(0), ElemId(1)] {
        let check = checks.iter().find(|check| check.elem == id).unwrap();
        assert!(
            !check.ok && check.qsu.is_nan() && check.shear_margin.is_nan(),
            "{check:?}"
        );
        let reason = check.unchecked_reason.as_deref().unwrap();
        assert!(reason.contains("増分解析を再実行してください"), "{reason}");
        assert_eq!(
            reason.contains("ultimate_clear_span_invalid"),
            id == ElemId(0),
            "{reason}"
        );
    }
}
