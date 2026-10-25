use super::*;
use approx::assert_relative_eq;
use sepika_core::ids::{ElemId, NodeId};
use sepika_core::model::RigidZone;
use sepika_material::uniaxial::Bilinear;

fn make_test_beam() -> crate::frame::beam::BeamElement {
    crate::frame::beam::BeamElement {
        id: ElemId(0),
        e: 205000.0,
        g: 78846.15,
        a: 80000.0,
        iy: 1.0666667e9,
        iz: 1.0666667e9,
        j: 0.0,
        as_y: 66666.67,
        as_z: 66666.67,
        length: 3000.0,
        mass_properties: sepika_core::model::SectionMassProperties::default(),
        mass_properties_error: None,
        nodes: [NodeId(0), NodeId(1)],
        axis: crate::transform::LocalFrame {
            rot: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        },
        rigid: RigidZone::default(),
        end_cond: [
            sepika_core::model::EndCondition::Fixed,
            sepika_core::model::EndCondition::Fixed,
        ],
        torsion_release: [false, false],
        eval_sections: vec![0.0, 0.5, 1.0],
        section: None,
        material: None,
        committed_disp: [0.0; 12],
        trial_disp: [0.0; 12],
        committed_member_loads: Vec::new(),
        trial_member_loads: Vec::new(),
        local_stiffness_cache: std::sync::OnceLock::new(),
    }
}

fn make_test_element() -> ConcentratedSpringBeam {
    let elastic = make_test_beam();
    let spring_i = Box::new(Bilinear::new(1.0e10, 1.0e20, 0.01));
    let spring_j = Box::new(Bilinear::new(1.0e10, 1.0e20, 0.01));
    ConcentratedSpringBeam::new_one_component(elastic, spring_i, spring_j)
}

fn make_yield_element() -> ConcentratedSpringBeam {
    let mut elastic = make_test_beam();
    elastic.iz = 1.0e8;
    elastic.iy = 1.0e8;
    let spring_i = Box::new(Bilinear::new(1.0e12, 1.0e7, 0.01));
    let spring_j = Box::new(Bilinear::new(1.0e12, 1.0e7, 0.01));
    ConcentratedSpringBeam::new_one_component(elastic, spring_i, spring_j)
}

/// Box<dyn UniaxialMaterial> から Bilinear の降伏値を読み出す（テスト用）。
fn spring_fy(spring: &dyn UniaxialMaterial) -> f64 {
    let mut b = Bilinear::new(1.0, 1.0, 0.0);
    b.deserialize_state(&spring.serialize_state()).unwrap();
    b.fy
}

#[test]
fn test_internal_force_no_double_count() {
    let mut elem = make_test_element();
    let ctx = Ctx {
        model: &sepika_core::model::Model::default(),
    };
    let du = LocalVec {
        data: smallvec::smallvec![0.0, 0.0, 0.0, 0.0, 0.0, 0.001, 0.0, 0.0, 0.0, 0.0, 0.0, -0.001],
    };
    elem.update_state(&du, true, &ctx).unwrap();
    let k = elem.tangent_stiffness(&ctx);
    let f = elem.internal_force(&ctx);
    let mut k_u = [0.0; 12];
    for i in 0..12 {
        let mut s = 0.0;
        for j in 0..12 {
            s += k.get(i, j) * elem.elastic.committed_disp[j];
        }
        k_u[i] = s;
    }
    for i in 0..12 {
        assert_relative_eq!(f.data[i], k_u[i], epsilon = 1.0);
    }
}

/// 材端ばねは強軸曲げ（局所 rz。DOF 5・11）にのみ作用し、軸・ねじり・弱軸曲げ
/// （rx・ry）には現れないこと。要素局所系は「y 軸＝断面のせい方向」規約のため、
/// 梁の鉛直曲げ（強軸）は rz まわりであり、塑性ヒンジもこの面に生じる。
#[test]
fn test_dof_only_rz() {
    let mut elem = make_test_element();
    let ctx = Ctx {
        model: &sepika_core::model::Model::default(),
    };
    let du = LocalVec {
        data: smallvec::smallvec![0.0, 0.0, 0.0, 0.0, 0.0, 0.001, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    };
    elem.update_state(&du, true, &ctx).unwrap();
    let f = elem.internal_force(&ctx);
    assert!(f.data[3].abs() < 1.0, "rx_i should not have spring moment");
    assert!(f.data[4].abs() < 1.0, "ry_i should not have spring moment");
    assert!(f.data[9].abs() < 1.0, "rx_j should not have spring moment");
    assert!(f.data[10].abs() < 1.0, "ry_j should not have spring moment");

    let k = elem.tangent_stiffness(&ctx);
    let k_sym = |i: usize, j: usize| {
        if k.get(i, j) != k.get(j, i) {
            (k.get(i, j) - k.get(j, i)).abs() < 1e-6
        } else {
            true
        }
    };
    for i in 0..12 {
        for j in 0..12 {
            assert!(k_sym(i, j), "K[{i}][{j}] != K[{j}][{i}]");
        }
    }
}

#[test]
fn test_spring_yield() {
    let mut elem = make_yield_element();
    let ctx = Ctx {
        model: &sepika_core::model::Model::default(),
    };

    let rot_yield = 1.0e7 / 1.0e12;
    let du_large = LocalVec {
        data: smallvec::smallvec![
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            rot_yield * 100.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0
        ],
    };

    let k_elastic = elem.tangent_stiffness(&ctx);

    elem.update_state(&du_large, false, &ctx).unwrap();
    let k_yielded = elem.tangent_stiffness(&ctx);

    let k55_elastic = k_elastic.get(5, 5);
    let k55_yielded = k_yielded.get(5, 5);
    assert!(
        k55_yielded < k55_elastic * 0.99,
        "yielded tangent should drop: elastic={} yielded={}",
        k55_elastic,
        k55_yielded
    );
}

#[test]
fn test_commit_revert() {
    let mut elem = make_test_element();
    let ctx = Ctx {
        model: &sepika_core::model::Model::default(),
    };
    let du = LocalVec {
        data: smallvec::smallvec![0.0, 0.0, 0.0, 0.0, 0.0, 0.001, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    };

    elem.update_state(&du, false, &ctx).unwrap();
    let g1 = elem.trial_rot_i;
    assert!(g1 > 0.0 && g1 < 0.001, "γ は節点回転の一部: {g1}");
    assert_relative_eq!(elem.trial_rot_i + elem.trial_thb_i, 0.001, epsilon = 1e-9);
    assert_relative_eq!(elem.rot_i, 0.0, epsilon = 1e-12);
    elem.revert_state();
    assert_relative_eq!(elem.trial_rot_i, 0.0, epsilon = 1e-12);
    assert_relative_eq!(elem.rot_i, 0.0, epsilon = 1e-12);

    elem.update_state(&du, false, &ctx).unwrap();
    elem.commit_state();
    assert_relative_eq!(elem.trial_rot_i, g1, epsilon = 1e-12);
    assert_relative_eq!(elem.rot_i, g1, epsilon = 1e-12);

    let du2 = LocalVec {
        data: smallvec::smallvec![0.0, 0.0, 0.0, 0.0, 0.0, 0.002, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    };
    elem.update_state(&du2, false, &ctx).unwrap();
    assert_relative_eq!(elem.trial_rot_i + elem.trial_thb_i, 0.003, epsilon = 1e-9);
    assert!(elem.trial_rot_i > g1, "節点回転の増加でばね変形も増える");
    elem.revert_state();
    assert_relative_eq!(elem.trial_rot_i, g1, epsilon = 1e-12);
    assert_relative_eq!(elem.rot_i, g1, epsilon = 1e-12);
}

#[test]
fn test_snapshot_restore() {
    let mut elem = make_test_element();
    let ctx = Ctx {
        model: &sepika_core::model::Model::default(),
    };
    let du = LocalVec {
        data: smallvec::smallvec![0.0, 0.0, 0.0, 0.0, 0.0, 0.001, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    };

    elem.update_state(&du, true, &ctx).unwrap();
    let g1 = elem.rot_i;
    assert!(g1 > 0.0, "コミット済みばね変形が非零");
    let snap = elem.snapshot_state();

    let du2 = LocalVec {
        data: smallvec::smallvec![0.0, 0.0, 0.0, 0.0, 0.0, 0.002, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    };
    elem.update_state(&du2, false, &ctx).unwrap();
    assert!(elem.trial_rot_i > g1, "トライアルばね変形が進む");

    elem.restore_state(&*snap);
    assert_relative_eq!(elem.rot_i, g1, epsilon = 1e-12);
    assert_relative_eq!(elem.trial_rot_i, g1, epsilon = 1e-12);
}

#[test]
fn test_condense_springs_zero_stiffness() {
    let beam = make_test_beam();
    let k_raw = beam.local_stiffness_raw();
    let k_pinned = condense_springs(&k_raw, 0.0, 0.0);
    let k_fixed = condense_springs(&k_raw, 1e30, 1e30);
    assert!(
        k_pinned.get(5, 5) < k_fixed.get(5, 5) * 0.5,
        "pinned rz_i should be much softer than fixed"
    );
}

#[test]
fn test_consistent_mass_uses_initial_end_spring_stiffness_after_yield() {
    let mut elastic = make_test_beam();
    elastic.mass_properties = sepika_core::model::SectionMassProperties::uniform(
        2.4e-9, elastic.a, elastic.iz, elastic.iy,
    );
    let mut elem = ConcentratedSpringBeam::new_one_component(
        elastic,
        Box::new(Bilinear::new(1.0e12, 1.0e7, 0.01)),
        Box::new(Bilinear::new(1.0e12, 1.0e7, 0.01)),
    );
    let before = elem.mass_matrix(MassOption::Consistent);
    let ctx = Ctx {
        model: &sepika_core::model::Model::default(),
    };
    elem.update_state(
        &LocalVec {
            data: smallvec::smallvec![0.0, 0.0, 0.0, 0.0, 0.0, 0.02, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        },
        false,
        &ctx,
    )
    .unwrap();
    assert!(elem.spring_i.probe(elem.trial_rot_i).1 < 1.0e12);
    let after = elem.mass_matrix(MassOption::Consistent);
    assert_eq!(before.data, after.data);

    let lumped_after = elem.mass_matrix(MassOption::Lumped);
    let mut beam_before = make_test_beam();
    beam_before.mass_properties = sepika_core::model::SectionMassProperties::uniform(
        2.4e-9,
        beam_before.a,
        beam_before.iz,
        beam_before.iy,
    );
    let lumped_before = ConcentratedSpringBeam::new_one_component(
        beam_before,
        Box::new(Bilinear::new(1.0e12, 1.0e7, 0.01)),
        Box::new(Bilinear::new(1.0e12, 1.0e7, 0.01)),
    )
    .mass_matrix(MassOption::Lumped);
    assert_eq!(lumped_before.data, lumped_after.data);
}

#[test]
fn rc_consistent_mass_resolver_matches_beam() {
    let rc_properties = sepika_core::model::SectionMassProperties::uniform(
        2.4e-9,
        80000.0,
        1.0666667e9,
        1.0666667e9,
    );
    let mut elastic = make_test_beam();
    elastic.mass_properties = rc_properties;
    let beam = elastic.clone();
    let concentrated = ConcentratedSpringBeam::new_one_component(
        elastic,
        Box::new(Bilinear::new(1.0e20, 1.0e30, 0.01)),
        Box::new(Bilinear::new(1.0e20, 1.0e30, 0.01)),
    );
    let concentrated_mass = concentrated.mass_matrix(MassOption::Consistent);
    let beam_mass = beam.mass_matrix(MassOption::Consistent);

    let concentrated_total = concentrated_mass.get(0, 0) + concentrated_mass.get(6, 6);
    let beam_total = beam_mass.get(0, 0) + beam_mass.get(6, 6);
    let concentrated_polar = concentrated_mass.get(3, 3) + concentrated_mass.get(9, 9);
    let beam_polar = beam_mass.get(3, 3) + beam_mass.get(9, 9);
    assert!(concentrated_total > 0.0);
    assert!(concentrated_polar > 0.0);
    assert_relative_eq!(concentrated_total, beam_total, max_relative = 1e-10);
    assert_relative_eq!(concentrated_polar, beam_polar, max_relative = 1e-10);
}

#[test]
fn test_condense_springs_singular_kbb_uses_kaa() {
    let k = LocalMat::zeros(12);
    let condensed = condense_springs(&k, 0.0, 5.0);
    assert_eq!(condensed.get(5, 5), 0.0);
    assert_eq!(condensed.get(11, 11), 5.0);
    assert!(condensed.data.iter().enumerate().all(|(index, value)| {
        let row = index / 12;
        let col = index % 12;
        (row, col) == (5, 5) && *value == 0.0
            || (row, col) == (11, 11) && *value == 5.0
            || *value == 0.0
    }));
}

#[test]
fn test_concentrated_spring_checkpoint_roundtrip() {
    let mut elem = make_test_element();
    let ctx = Ctx {
        model: &sepika_core::model::Model::default(),
    };
    let du = LocalVec {
        data: smallvec::smallvec![0.0, 0.0, 0.0, 0.0, 0.0, 0.001, 0.0, 0.0, 0.0, 0.0, 0.0, -0.0005],
    };
    elem.update_state(&du, true, &ctx).unwrap();

    let checkpoint = elem.serialize_checkpoint();
    let mut restored = make_test_element();
    restored.deserialize_checkpoint(&checkpoint).unwrap();
    assert_eq!(checkpoint, restored.serialize_checkpoint());
    assert_eq!(
        elem.internal_force(&ctx).data,
        restored.internal_force(&ctx).data
    );
    assert_eq!(
        elem.state_member_forces(&ctx),
        restored.state_member_forces(&ctx)
    );
}
#[test]
fn test_mn_interaction_reduces_spring_yield() {
    let my0 = 1.0e7;
    let elastic = make_test_beam();
    let ea_over_l = 205000.0 * 80000.0 / 3000.0;
    let n_allow = ea_over_l;
    let spring_i = Box::new(Bilinear::new(1.0e12, my0, 0.01));
    let spring_j = Box::new(Bilinear::new(1.0e12, my0, 0.01));
    let mut elem = ConcentratedSpringBeam::new_one_component(elastic, spring_i, spring_j)
        .with_mn_interaction(my0, n_allow);

    let ctx = Ctx {
        model: &sepika_core::model::Model::default(),
    };
    let du = LocalVec {
        data: smallvec::smallvec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -0.5, 0.0, 0.0, 0.0, 0.0, 0.0],
    };
    elem.update_state(&du, false, &ctx).unwrap();
    assert_relative_eq!(spring_fy(&*elem.spring_i), 0.5 * my0, max_relative = 1e-9);
    assert_relative_eq!(spring_fy(&*elem.spring_j), 0.5 * my0, max_relative = 1e-9);

    let du_t = LocalVec {
        data: smallvec::smallvec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0],
    };
    elem.update_state(&du_t, false, &ctx).unwrap();
    assert_relative_eq!(spring_fy(&*elem.spring_i), my0, max_relative = 1e-9);

    let elastic_t = make_test_beam();
    let spring_ti = Box::new(Bilinear::new(1.0e12, my0, 0.01));
    let spring_tj = Box::new(Bilinear::new(1.0e12, my0, 0.01));
    let mut elem_t = ConcentratedSpringBeam::new_one_component(elastic_t, spring_ti, spring_tj)
        .with_mn_interaction(my0, n_allow);
    elem_t.update_state(&du_t, false, &ctx).unwrap();
    assert_relative_eq!(spring_fy(&*elem_t.spring_i), 0.5 * my0, max_relative = 1e-9);
}

#[test]
fn test_mn_interaction_disabled_keeps_yield() {
    let my0 = 1.0e7;
    let elastic = make_test_beam();
    let spring_i = Box::new(Bilinear::new(1.0e12, my0, 0.01));
    let spring_j = Box::new(Bilinear::new(1.0e12, my0, 0.01));
    let mut elem = ConcentratedSpringBeam::new_one_component(elastic, spring_i, spring_j);
    let ctx = Ctx {
        model: &sepika_core::model::Model::default(),
    };
    let du = LocalVec {
        data: smallvec::smallvec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -0.5, 0.0, 0.0, 0.0, 0.0, 0.0],
    };
    elem.update_state(&du, false, &ctx).unwrap();
    assert_relative_eq!(spring_fy(&*elem.spring_i), my0, max_relative = 1e-12);
}

#[test]
fn test_mn_interaction_floor_at_high_axial() {
    let my0 = 1.0e7;
    let elastic = make_test_beam();
    let ea_over_l = 205000.0 * 80000.0 / 3000.0;
    let spring_i = Box::new(Bilinear::new(1.0e12, my0, 0.01));
    let spring_j = Box::new(Bilinear::new(1.0e12, my0, 0.01));
    let mut elem = ConcentratedSpringBeam::new_one_component(elastic, spring_i, spring_j)
        .with_mn_interaction(my0, ea_over_l);
    let ctx = Ctx {
        model: &sepika_core::model::Model::default(),
    };
    let du = LocalVec {
        data: smallvec::smallvec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -3.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    };
    elem.update_state(&du, false, &ctx).unwrap();
    assert_relative_eq!(spring_fy(&*elem.spring_i), 0.02 * my0, max_relative = 1e-9);
}

#[test]
fn test_mn_interaction_yield_moment_in_response() {
    let my0 = 1.0e7;
    let elastic = make_test_beam();
    let ea_over_l = 205000.0 * 80000.0 / 3000.0;
    let spring_i = Box::new(Bilinear::new(1.0e12, my0, 0.0));
    let spring_j = Box::new(Bilinear::new(1.0e12, my0, 0.0));
    let mut elem = ConcentratedSpringBeam::new_one_component(elastic, spring_i, spring_j)
        .with_mn_interaction(my0, ea_over_l);
    let ctx = Ctx {
        model: &sepika_core::model::Model::default(),
    };
    let du = LocalVec {
        data: smallvec::smallvec![0.0, 0.0, 0.0, 0.0, 0.1, 0.0, -0.5, 0.0, 0.0, 0.0, 0.0, 0.0],
    };
    elem.update_state(&du, false, &ctx).unwrap();
    let (m, _) = elem.spring_i.clone_box().trial(0.1);
    assert_relative_eq!(m, 0.5 * my0, max_relative = 1e-6);
}

/// 材端ばねは**強軸**（要素局所 rz。断面のせい方向の曲げ）に作用すること。
///
/// 要素局所系は「y 軸＝断面のせい方向」規約のため、強軸曲げは (uy, rz) ブロック
/// （`beam/construct.rs` のクロス変換で断面 iy＝強軸が要素 iz に入る）。
/// 骨格の降伏モーメント（強軸 Zp·σy）と初期剛性（6EI/l、I は強軸）も強軸で与える
/// ため、ばねを弱軸（ry）に入れると強軸曲げが永久に降伏せず保有水平耐力を
/// 過大評価する（危険側）。強軸/弱軸で剛性が大きく異なる断面で確認する。
#[test]
fn test_spring_acts_on_strong_axis_rz() {
    let mut beam = make_test_beam();
    beam.iz = 1.0e9;
    beam.iy = 1.0e8;
    let k_raw = beam.local_stiffness_raw();
    let k_stiff = condense_springs(&k_raw, 1e30, 1e30);
    let k_soft = condense_springs(&k_raw, 1.0, 1.0);

    for &dof in &[5usize, 11] {
        assert!(
            k_soft.get(dof, dof) < k_stiff.get(dof, dof) * 0.5,
            "強軸 DOF {dof} にばねが効いていない: soft={} stiff={}",
            k_soft.get(dof, dof),
            k_stiff.get(dof, dof)
        );
    }
    for &dof in &[3usize, 4, 9, 10] {
        assert_relative_eq!(k_soft.get(dof, dof), k_stiff.get(dof, dof), epsilon = 1.0);
    }
}

/// 材端ばねが十分に剛なら、`compute_kstar` は弾性梁の `local_stiffness()` と一致する。
/// 剛域・端部条件（ピン）を持つ部材でも一致すること＝可撓長と端部条件が
/// 弾性梁と同じ土台で組まれていることの検証。
#[test]
fn test_compute_kstar_matches_elastic_beam_with_rigid_zone_and_pin() {
    use sepika_core::model::EndCondition;

    let cases: [(RigidZone, [EndCondition; 2], &str); 4] = [
        (
            RigidZone::default(),
            [EndCondition::Fixed; 2],
            "剛域なし・剛接",
        ),
        (
            RigidZone {
                length_i: 300.0,
                length_j: 200.0,
                ..Default::default()
            },
            [EndCondition::Fixed; 2],
            "剛域あり・剛接",
        ),
        (
            RigidZone::default(),
            [EndCondition::Pinned, EndCondition::Fixed],
            "剛域なし・i端ピン",
        ),
        (
            RigidZone {
                length_i: 300.0,
                length_j: 200.0,
                ..Default::default()
            },
            [EndCondition::Pinned, EndCondition::Fixed],
            "剛域あり・i端ピン",
        ),
    ];

    for (rigid, end_cond, label) in cases {
        let mut beam = make_test_beam();
        beam.rigid = rigid;
        beam.end_cond = end_cond;

        let k_ref = beam.local_stiffness();
        let k = compute_kstar(&beam, &beam.local_stiffness_flex(), 1e18, 1e18);
        for i in 0..12 {
            for j in 0..12 {
                let scale = k_ref.get(i, j).abs().max(1.0);
                assert!(
                    (k.get(i, j) - k_ref.get(i, j)).abs() <= scale * 1e-6,
                    "{label}: K[{i}][{j}] が弾性梁と不一致: {} vs {}",
                    k.get(i, j),
                    k_ref.get(i, j)
                );
            }
        }
    }
}

/// `state_member_forces` が復元力（`internal_force`）と整合した内力分布を返すこと。
#[test]
fn test_state_member_forces_matches_internal_force() {
    let mut elem = make_test_element();
    let ctx = Ctx {
        model: &sepika_core::model::Model::default(),
    };
    let du = LocalVec {
        data: smallvec::smallvec![0.0, 0.0, 0.0, 0.0, 0.0, 0.001, 0.0, 0.0, 0.0, 0.0, 0.0, -0.001],
    };
    elem.update_state(&du, true, &ctx).unwrap();

    let mf = elem
        .state_member_forces(&ctx)
        .expect("材端集中ばね梁は state_member_forces を実装しているはず");
    assert_eq!(mf.at.len(), 3, "評価断面数（0/0.5/1.0）と一致するはず");

    let f = elem.internal_force(&ctx);
    let arr: [f64; 12] = std::array::from_fn(|i| f.data[i]);
    let f_local = elem.elastic.axis.rotate_to_local(&arr);
    let mz_i = mf.at[0].1[5];
    let mz_j = mf.at[2].1[5];
    assert_relative_eq!(mz_i, -f_local[5], epsilon = 1e-6);
    assert_relative_eq!(mz_j, f_local[11], epsilon = 1e-6);
    assert!(
        mz_i.abs() > 1.0,
        "曲げを与えたのでモーメントは非ゼロのはず: {}",
        mz_i
    );
}

#[test]
fn explicit_total_reference_subtracts_elastic_rotation_without_penalty() {
    let mut elastic = make_test_beam();
    elastic.e = 1000.0;
    elastic.iz = 1e9;
    elastic.length = 6000.0;
    let total = sepika_material::HysteresisRule::Retrograde {
        crack: (250000.0, 0.00025),
        yield_point: (1e6, 0.004),
        ultimate: (1.1e6, 0.016),
    };
    let mk = || {
        Box::new(sepika_material::HysteresisMaterial::new(total.clone()))
            as Box<dyn UniaxialMaterial>
    };
    let mut element = ConcentratedSpringBeam::new_one_component(elastic, mk(), mk())
        .with_total_rotation_reference(1e9);
    let model = sepika_core::model::Model::default();
    let ctx = Ctx { model: &model };
    let elastic_k = element.elastic.local_stiffness_flex_rc_reference();
    let initial = element.tangent_stiffness(&ctx);
    assert_eq!(initial.data, elastic_k.data);
    assert_relative_eq!(initial.get(5, 5) + initial.get(5, 11), 1e9, epsilon = 1e-6);
    assert_eq!(
        element.mass_matrix(MassOption::Consistent).data,
        element.elastic.mass_matrix(MassOption::Consistent).data
    );
    let du = |theta| LocalVec {
        data: smallvec::smallvec![0.0, 0.0, 0.0, 0.0, 0.0, theta, 0.0, 0.0, 0.0, 0.0, 0.0, theta],
    };
    element.update_state(&du(0.0001), true, &ctx).unwrap();
    assert!(element
        .end_spring_rotations()
        .unwrap()
        .iter()
        .all(|r| r.abs() < 1e-15));
    element.update_state(&du(0.0039), true, &ctx).unwrap();
    assert_relative_eq!(
        element.internal_force(&ctx).data[5],
        1e6,
        max_relative = 1e-9
    );
    assert_relative_eq!(element.thb_i, 0.001, epsilon = 1e-12);
    assert_relative_eq!(element.rot_i, 0.004, epsilon = 1e-12);
    assert_relative_eq!(
        element.end_spring_rotations().unwrap()[0],
        0.003,
        epsilon = 1e-12
    );
    let saved = element.serialize_checkpoint();
    let snapshot = element.snapshot_state();
    element.update_state(&du(0.08), false, &ctx).unwrap();
    assert!(element
        .tangent_stiffness(&ctx)
        .data
        .iter()
        .all(|v| v.is_finite()));
    element.revert_state();
    assert_relative_eq!(
        element.end_spring_rotations().unwrap()[0],
        0.003,
        epsilon = 1e-12
    );
    element.update_state(&du(0.08), true, &ctx).unwrap();
    assert!(element
        .tangent_stiffness(&ctx)
        .data
        .iter()
        .all(|v| v.is_finite()));
    element.update_state(&du(-0.1), true, &ctx).unwrap();
    assert!(element
        .internal_force(&ctx)
        .data
        .iter()
        .all(|v| v.is_finite()));
    element.restore_state(snapshot.as_ref());
    assert_relative_eq!(
        element.internal_force(&ctx).data[5],
        1e6,
        max_relative = 1e-9
    );
    element.deserialize_checkpoint(&saved).unwrap();
    assert_relative_eq!(
        element.end_spring_rotations().unwrap()[0],
        0.003,
        epsilon = 1e-12
    );
    let alpha_one = sepika_material::HysteresisRule::Retrograde {
        crack: (250000.0, 0.00025),
        yield_point: (1e6, 0.001),
        ultimate: (1.1e6, 0.004),
    };
    let mk = || {
        Box::new(sepika_material::HysteresisMaterial::new(alpha_one.clone()))
            as Box<dyn UniaxialMaterial>
    };
    let mut element = ConcentratedSpringBeam::new_one_component(element.elastic, mk(), mk())
        .with_total_rotation_reference(1e9);
    element.elastic.committed_disp = [0.0; 12];
    element.elastic.trial_disp = [0.0; 12];
    element.update_state(&du(0.001), true, &ctx).unwrap();
    assert!(element
        .end_spring_rotations()
        .unwrap()
        .iter()
        .all(|r| r.abs() < 1e-15));
    assert_relative_eq!(
        element.internal_force(&ctx).data[5],
        1e6,
        max_relative = 1e-9
    );
}

#[test]
fn explicit_total_reference_preserves_takeda_unloading_values() {
    let mut elastic = make_test_beam();
    elastic.e = 1000.0;
    elastic.iz = 1e9;
    elastic.length = 6000.0;
    let total = sepika_material::HysteresisRule::Takeda {
        crack: (250000.0, 0.00025),
        yield_point: (1e6, 0.004),
        ultimate: (1.1e6, 0.016),
        alpha: 0.4,
    };
    let mk = || {
        Box::new(sepika_material::HysteresisMaterial::new(total.clone()))
            as Box<dyn UniaxialMaterial>
    };
    let mut element = ConcentratedSpringBeam::new_one_component(elastic, mk(), mk())
        .with_total_rotation_reference(1e9);
    let model = sepika_core::model::Model::default();
    let ctx = Ctx { model: &model };
    let du = |theta| LocalVec {
        data: smallvec::smallvec![0.0, 0.0, 0.0, 0.0, 0.0, theta, 0.0, 0.0, 0.0, 0.0, 0.0, theta],
    };
    // 2Ryまで載荷した武田型の除荷勾配 Ku=S*(Rmax/Ry)^(-.4)。
    element.update_state(&du(0.008), true, &ctx).unwrap();
    assert_relative_eq!(
        element.internal_force(&ctx).data[5],
        1_033_333.333_333_333_3,
        max_relative = 1e-10
    );
    element.update_state(&du(-0.0001), true, &ctx).unwrap();
    assert_relative_eq!(
        element.internal_force(&ctx).data[5],
        957_547.505_007_813_4,
        max_relative = 1e-10
    );
    assert_relative_eq!(
        element.end_spring_rotations().unwrap()[0],
        0.006_942_452_494_992_187,
        epsilon = 1e-12
    );
    let tangent = element.tangent_stiffness(&ctx);
    assert_relative_eq!(
        tangent.get(5, 5) + tangent.get(5, 11),
        757_858_283.255_199,
        max_relative = 1e-10
    );
}

fn loaded_udl(w: f64, axis: usize) -> sepika_core::model::MemberLoad {
    let mut direction = [0.0; 3];
    direction[axis] = 1.0;
    sepika_core::model::MemberLoad::manual(
        ElemId(0),
        direction,
        sepika_core::model::MemberLoadKind::Distributed {
            a: 0.0,
            b: 3000.0,
            w1: w,
            w2: w,
        },
    )
}

fn close_loaded(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-7 * expected.abs().max(1.0),
        "{actual} != {expected}"
    );
}

#[test]
fn member_load_equilibrates_elastic_end_connections_and_section_forces() {
    use sepika_core::model::EndCondition;
    let ctx = Ctx {
        model: &Default::default(),
    };
    let zero = LocalVec {
        data: smallvec::smallvec![0.0; 12],
    };
    for condition in [
        EndCondition::Fixed,
        EndCondition::Pinned,
        EndCondition::SemiRigid { k_theta: 2e10 },
    ] {
        let mut elem = make_test_element();
        elem.elastic.end_cond = [condition; 2];
        let kb = 2.0 * elem.elastic.e * elem.elastic.iz / 3000.0;
        let moment = match condition {
            EndCondition::Fixed => 750000.0 / (1.0 + kb / 1e10),
            EndCondition::Pinned => 0.0,
            EndCondition::SemiRigid { k_theta } => 750000.0 / (1.0 + kb / 1e10 + kb / k_theta),
        };
        for factor in [1.0, 0.5, 1.0, 0.0] {
            let loads = if factor == 0.0 {
                vec![]
            } else {
                vec![loaded_udl(factor, 1)]
            };
            elem.update_state_with_member_loads(&zero, &loads, true, &ctx)
                .unwrap();
            let response = elem.state_frame_load_response().unwrap().unwrap();
            let f = response.physical_frame_end_force_global;
            close_loaded(f[1], -1500.0 * factor);
            close_loaded(f[7], -1500.0 * factor);
            close_loaded(f[5], -moment * factor);
            close_loaded(f[11], moment * factor);
            close_loaded(elem.spring_i.probe(elem.trial_rot_i).0, f[5]);
            close_loaded(elem.spring_j.probe(elem.trial_rot_j).0, f[11]);
            close_loaded(
                response.section.at(0.5).unwrap()[5],
                moment * factor - 1125000.0 * factor,
            );
            assert_eq!(elem.internal_force(&ctx).data, response.nodal_residual.data);
        }
    }
}

#[test]
fn member_point_load_yields_springs_and_matches_bilinear_equilibrium_and_tangent() {
    let mut elem = make_yield_element();
    let ctx = Ctx {
        model: &Default::default(),
    };
    let zero = LocalVec {
        data: smallvec::smallvec![0.0; 12],
    };
    let load = sepika_core::model::MemberLoad::manual(
        ElemId(0),
        [0.0, 1.0, 0.0],
        sepika_core::model::MemberLoadKind::Point { a: 1500.0, p: 1e5 },
    );
    elem.update_state_with_member_loads(&zero, std::slice::from_ref(&load), false, &ctx)
        .unwrap();
    let kb = 2.0 * 205000.0 * 1e8 / 3000.0;
    let rotation = (37500000.0 - 1e7 + 1e10 * 1e-5) / (kb + 1e10);
    let moment = 37500000.0 - kb * rotation;
    close_loaded(-elem.trial_rot_i, rotation);
    close_loaded(elem.trial_rot_j, rotation);
    close_loaded(elem.internal_force(&ctx).data[5], -moment);
    close_loaded(elem.internal_force(&ctx).data[11], moment);
    assert!(moment > 1e7);
    let tangent = elem.tangent_stiffness(&ctx);
    let before = elem.internal_force(&ctx);
    let snapshot = elem.snapshot_state();
    let mut increment = LocalVec {
        data: smallvec::smallvec![0.0; 12],
    };
    increment.data[5] = 1e-8;
    elem.update_state_with_member_loads(&increment, std::slice::from_ref(&load), false, &ctx)
        .unwrap();
    let after = elem.internal_force(&ctx);
    close_loaded((after.data[5] - before.data[5]) / 1e-8, tangent.get(5, 5));
    elem.restore_state(snapshot.as_ref());
    elem.commit_state();
    let checkpoint = elem.serialize_checkpoint();
    let mut resumed = make_yield_element();
    resumed.deserialize_checkpoint(&checkpoint).unwrap();
    assert_eq!(resumed.serialize_checkpoint(), checkpoint);
    for next in [&mut elem, &mut resumed] {
        next.update_state_with_member_loads(&zero, &[], false, &ctx)
            .unwrap();
        assert!(next.internal_force(&ctx).data[5].abs() > 1e6);
        next.revert_state();
        assert_eq!(next.serialize_checkpoint(), checkpoint);
    }
}

#[test]
fn member_load_failed_trial_and_checkpoint_restore_every_material_and_load_state() {
    let ctx = Ctx {
        model: &Default::default(),
    };
    let zero = LocalVec {
        data: smallvec::smallvec![0.0; 12],
    };
    let mut elem = make_yield_element();
    elem.update_state_with_member_loads(&zero, &[loaded_udl(100.0, 1)], true, &ctx)
        .unwrap();
    let committed = elem.serialize_checkpoint();
    elem.update_state_with_member_loads(&zero, &[loaded_udl(50.0, 1)], false, &ctx)
        .unwrap();
    let trial = elem.serialize_checkpoint();
    for invalid in [loaded_udl(f64::NAN, 1), loaded_udl(f64::MAX, 1), {
        let mut load = loaded_udl(1.0, 1);
        load.elem = ElemId(9);
        load
    }] {
        assert!(elem
            .update_state_with_member_loads(&zero, &[invalid], true, &ctx)
            .is_err());
        assert_eq!(elem.serialize_checkpoint(), trial);
    }
    let mut cp: ConcentratedSpringCheckpoint = bincode::deserialize(&trial).unwrap();
    cp.spring_j = vec![1, 2];
    assert!(elem
        .deserialize_checkpoint(&bincode::serialize(&cp).unwrap())
        .is_err());
    assert_eq!(elem.serialize_checkpoint(), trial);
    elem.revert_state();
    assert_eq!(elem.serialize_checkpoint(), committed);
}

#[test]
fn axial_member_load_sets_each_end_mn_limit_from_its_physical_axial_force() {
    let mut elem = make_yield_element().with_mn_interaction(1e7, 10000.0);
    let ctx = Ctx {
        model: &Default::default(),
    };
    let mut increment = LocalVec {
        data: smallvec::smallvec![0.0; 12],
    };
    increment.data[6] = 3000.0 * 3000.0 / (205000.0 * 80000.0);
    elem.update_state_with_member_loads(&increment, &[loaded_udl(2.0, 0)], true, &ctx)
        .unwrap();
    close_loaded(spring_fy(elem.spring_i.as_ref()), 4e6);
    close_loaded(spring_fy(elem.spring_j.as_ref()), 1e7);
    let response = elem.state_frame_load_response().unwrap().unwrap();
    for (x, n) in [(0.0, 6000.0), (0.5, 3000.0), (1.0, 0.0)] {
        close_loaded(response.section.at(x).unwrap()[0], n);
    }
    close_loaded(response.nodal_residual.data[0], -6000.0);
    close_loaded(response.nodal_residual.data[6], 0.0);
    let committed = elem.serialize_checkpoint();
    let zero = LocalVec {
        data: smallvec::smallvec![0.0; 12],
    };
    elem.update_state_with_member_loads(&zero, &[], false, &ctx)
        .unwrap();
    close_loaded(spring_fy(elem.spring_i.as_ref()), 7e6);
    close_loaded(spring_fy(elem.spring_j.as_ref()), 7e6);
    elem.revert_state();
    close_loaded(spring_fy(elem.spring_i.as_ref()), 4e6);
    close_loaded(spring_fy(elem.spring_j.as_ref()), 1e7);
    assert_eq!(elem.serialize_checkpoint(), committed);
}

#[test]
fn rigid_zone_and_total_rotation_reference_keep_loaded_material_equilibrium() {
    let ctx = Ctx {
        model: &Default::default(),
    };
    let zero = LocalVec {
        data: smallvec::smallvec![0.0; 12],
    };
    for total in [false, true] {
        let mut elem = make_yield_element();
        elem.elastic.rigid = sepika_core::model::RigidZone {
            length_i: 200.0,
            length_j: 400.0,
            ..Default::default()
        };
        if total {
            elem = elem.with_total_rotation_reference(1e12);
        }
        elem.update_state_with_member_loads(&zero, &[loaded_udl(100.0, 1)], false, &ctx)
            .unwrap();
        let response = elem.state_frame_load_response().unwrap().unwrap();
        let f = response.physical_frame_end_force_global;
        close_loaded(f[1] + f[7], -300000.0);
        close_loaded(f[5] + f[11] + 3000.0 * f[7], -450000000.0);
        close_loaded(
            response.flex_face_force_local[5],
            elem.spring_i.probe(elem.trial_rot_i).0,
        );
        close_loaded(
            response.flex_face_force_local[11],
            elem.spring_j.probe(elem.trial_rot_j).0,
        );
    }
}
