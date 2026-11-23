use super::*;
use crate::app::rc_wall_side_column_tests::fixture as rc_wall_side_column_model;

#[path = "../../../../../../sepika-solver/tests/support/src_wall_model.rs"]
mod src_wall_model;

fn test_face_limits() -> [SrcFrameFaceLimits; 2] {
    std::array::from_fn(|_| SrcFrameFaceLimits {
        axial_kn: 8_000.0,
        bending_strong_knm: 2_000.0,
        bending_weak_knm: 2_000.0,
        shear_strong_kn: 1_000.0,
        shear_weak_kn: 1_000.0,
    })
}

#[test]
fn src_holding_requires_bending_at_the_unembedded_lower_face() {
    let model = src_wall_model::analysis_input_model(false);
    let mut faces = [[0.0; 6]; 2];
    faces[1][5] = 2.0e9;
    let limits = test_face_limits();
    let error =
        check_unembedded_base_bending(&model.elements[0], &model, &faces, &limits).unwrap_err();
    assert!(error.contains("下側の物理フェースが曲げ耐力に達していません"));
    faces.swap(0, 1);
    check_unembedded_base_bending(&model.elements[0], &model, &faces, &limits).unwrap();
}

#[test]
fn src_holding_same_run_rejects_the_later_point_after_either_purpose_reaches_shear() {
    let reached = FrameUtilization {
        shear: 1.0,
        bending: 0.1,
    };
    let below = FrameUtilization {
        shear: 0.4,
        bending: 0.3,
    };
    for reason in [
        check_adoption_order(ElemId(2), 3, &reached, 4, &below).unwrap_err(),
        check_adoption_order(ElemId(2), 4, &below, 3, &reached).unwrap_err(),
    ] {
        assert!(reason.contains("部材 ID 2"));
        assert!(reason.contains("step 3"));
    }
    check_adoption_order(ElemId(2), 3, &reached, 3, &reached).unwrap();
    check_adoption_order(ElemId(2), 3, &below, 4, &reached).unwrap();
}

#[test]
fn src_holding_accepts_an_overall_wall_base_mechanism_from_the_lowest_story_wall_lower_face() {
    let story = |wall| StoryMembers {
        columns: BTreeSet::new(),
        frame_columns: BTreeSet::new(),
        walls: BTreeSet::from([ElemId(wall)]),
    };
    let stories = [story(4), story(5)];
    let mut origin = SrcCollapseOrigin {
        elem: Some(ElemId(4)),
        end: Some(SrcMemberEnd::I),
        failure: Some(SrcFailure::Bending),
        basis: "連層壁の壁脚曲げによる全体崩壊の機構図".into(),
    };
    let base_only = BTreeSet::from([0]);
    check_overall_mechanism_evidence(
        &stories,
        std::slice::from_ref(&origin),
        &base_only,
        true,
        false,
    )
    .unwrap();
    assert!(check_overall_mechanism_evidence(
        &stories,
        std::slice::from_ref(&origin),
        &base_only,
        false,
        false
    )
    .is_err());
    origin.end = Some(SrcMemberEnd::J);
    assert!(check_overall_mechanism_evidence(
        &stories,
        std::slice::from_ref(&origin),
        &base_only,
        true,
        false
    )
    .is_err());
    origin.end = Some(SrcMemberEnd::I);
    origin.elem = Some(ElemId(5));
    assert!(check_overall_mechanism_evidence(
        &stories,
        &[origin],
        &BTreeSet::from([1]),
        true,
        false
    )
    .is_err());
}

#[test]
fn src_holding_unused_wall_plate_does_not_change_the_active_frame_type() {
    let mut app = App::default();
    let mut model = crate::sample::portal_frame();
    let unused = src_wall_model::analysis_input_model(true)
        .wall_plates
        .remove(0);
    model.wall_plates.push(unused);
    app.core.model = model;
    assert!(!app.has_src_holding_members());
}

#[test]
fn src_holding_rejects_columns_split_at_the_story_cut_even_with_an_unsplit_column() {
    use sepika_core::ids::NodeId;
    use sepika_solver::nonlinear::pushover::story_response::{CutForce, StoryCut};

    let mut model = src_wall_model::analysis_input_model(false);
    let mut middle = model.nodes[2].clone();
    middle.id = NodeId(model.nodes.len() as u32);
    middle.coord[2] = 3500.0;
    middle.story = None;
    let mut upper = model.elements[0].clone();
    upper.id = ElemId(model.elements.len() as u32);
    upper.nodes[0] = middle.id;
    model.elements[0].nodes[1] = middle.id;
    model.nodes.push(middle);
    let upper_id = upper.id;
    model.elements.push(upper);
    let response = ConfirmedStepResponse {
        run_id: "切断面分割試験".into(),
        input_generation: Some(vec![1]),
        direction: SeismicDir::X,
        sign: PushoverSign::Positive,
        step: 1,
        members: vec![],
        src_walls: vec![],
        src_frame_faces: vec![],
        cuts: vec![StoryCut {
            layer: 0,
            elevation_mm: 3500.0,
            forces: vec![CutForce {
                elem: ElemId(1),
                group: ForceGroup::Frame,
                force_n: 1000.0,
            }],
            external_n: 1000.0,
            reference_n: 1000.0,
            support_n: 0.0,
            tolerance_n: 0.01,
            unavailable: None,
        }],
    };
    let reason = story_members(&model, &response)
        .err()
        .expect("所属を確定できない柱");
    assert!(
        reason.contains(&format!("SRC柱 ID 0、{}", upper_id.0)),
        "{reason}"
    );
    assert!(reason.contains("所属層を一意に確定できません"), "{reason}");
}

#[test]
fn unembedded_src_base_uses_rc_effective_depth_in_tensile_reinforcement_ratio() {
    use sepika_core::section_shape::RebarSize;

    let mut model = src_wall_model::analysis_input_model(false);
    let Some(SectionShape::SrcColumnRect { rebar, .. }) = model.sections[0].shape.as_mut() else {
        panic!("SRC柱断面が必要");
    };
    let bar_diameter_mm = (4.0_f64 * 450.0 / std::f64::consts::PI).sqrt();
    rebar.main_dia = RebarSize::DiameterMm(bar_diameter_mm);
    rebar.x = vec![6];
    rebar.y = vec![2];
    rebar.cover = 60.0 - rebar.hoop.dia.designation_or_diameter_mm() - bar_diameter_mm / 2.0;
    let rank = rc_rank_for_unembedded_base(
        &model.elements[0],
        &model,
        &[[0.0; 6]; 2],
        &SrcColumnConfirmation {
            brittle_rc: Some(false),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(rank, MemberRank::FB);
}

#[test]
fn src_holding_wall_near_limit_cannot_use_non_shear_failure_in_either_purpose() {
    use sepika_core::src_wall_capacity::SrcWallEndForces;

    for demand_n in [99.99995, 100.00005] {
        let review = SrcWallShearReview {
            elem: ElemId(4),
            ends: Some(
                [SrcWallEndForces {
                    shear_n: demand_n,
                    ..Default::default()
                }; 2],
            ),
            demand_n: Some(demand_n),
            limit_n: Some(100.0),
            unavailable: None,
        };
        let shear_reached = review.reached_limit().unwrap();
        assert!(wall_failure_matches(
            SrcFailure::Shear,
            shear_reached,
            false
        ));
        assert!(!wall_failure_matches(
            SrcFailure::Other,
            shear_reached,
            false
        ));
        assert!(!wall_failure_matches(
            SrcFailure::Bending,
            shear_reached,
            true
        ));
    }
}

fn fixture() -> App {
    use sepika_core::ids::NodeId;

    let mut app = App::default();
    let mut model = src_wall_model::analysis_input_model(false);
    for mut node in model.nodes.clone() {
        node.id = NodeId(node.id.0 + 4);
        node.coord[1] += 4_000.0;
        model.stories[node.story.unwrap().index()]
            .node_ids
            .push(node.id);
        model.nodes.push(node);
    }
    for mut member in model.elements.clone() {
        member.id = ElemId(member.id.0 + 4);
        for node in &mut member.nodes {
            node.0 += 4;
        }
        model.elements.push(member);
    }
    for load in &mut model.load_cases[0].nodal {
        load.values[2] = -400_000.0;
    }
    let extra_gravity: Vec<_> = model.load_cases[0]
        .nodal
        .iter()
        .cloned()
        .map(|mut load| {
            load.node.0 += 4;
            load
        })
        .collect();
    model.load_cases[0].nodal.extend(extra_gravity);
    for beam in [ElemId(2), ElemId(6)] {
        model.load_cases[0]
            .member
            .push(sepika_core::model::MemberLoad::full_length_uniform(
                beam,
                [0.0, 0.0, -1.0],
                2200.0,
                5.0,
            ));
    }
    assert!(app.load_model(model));
    app.core.design_frame = FrameType::SrcFrame;
    app.core.analysis_cfg.threads = 1;
    app.core.analysis_cfg.push_steps = 4;
    app.core.analysis_cfg.push_use_max_disp = true;
    app.core.analysis_cfg.push_use_drift_angle = false;
    app.core.analysis_cfg.push_max_disp = 0.5;
    app.run_preparation();
    let wall = &app.core.model.wall_plates[0];
    let detailing = sepika_edit::SetWallPlateAttrs {
        src_wall_detailing_confirmed: true,
        src_steel_plate: wall.src_steel_plate,
        id: wall.id,
        opening_area: wall.opening_area,
        opening_weight: wall.opening_weight,
        openings: wall.openings.clone(),
        dl_support: wall.dl_support,
        self_weight_shares: wall.self_weight_shares.clone(),
        loads: wall.loads.clone(),
        slit: wall.slit,
    };
    app.core
        .scoped
        .undo
        .run(&mut app.core.model, Box::new(detailing));
    assert!(app.core.model.wall_plates[0].src_wall_detailing_confirmed);
    app.run_combination(0);
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    for dir in [SeismicDir::X, SeismicDir::Y] {
        app.run_seismic(dir);
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
    }
    app
}

fn save_case(app: &mut App, direction: SeismicDir, sign: PushoverSign) {
    assert!(app.core.model.wall_plates[0].src_wall_detailing_confirmed);
    app.start_src_pushover_job(direction, sign);
    crate::app::tests::wait_for_job(app);
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let results = app.core.scoped.results.as_mut().unwrap();
    let run = match (direction, sign) {
        (SeismicDir::X, PushoverSign::Positive) => results.pushover_x.as_mut(),
        (SeismicDir::Y, PushoverSign::Positive) => results.pushover_y.as_mut(),
        (_, PushoverSign::Negative) => results
            .src_pushover_negative
            .iter_mut()
            .find(|(d, _, _)| *d == direction)
            .map(|(_, _, p)| p),
    }
    .unwrap();
    let step = run.capacity_curve.iter().find(|p| p.step > 0).unwrap().step;
    let origin_id = ElemId(6);
    let record = run
        .confirmed_history
        .as_ref()
        .unwrap()
        .iter()
        .find(|r| r.step == step)
        .unwrap();
    let origin_faces = frame_faces(&record.src_frame_faces, origin_id).unwrap();
    let anchor_columns: &[u32] = if direction == SeismicDir::X {
        &[4, 5]
    } else {
        &[0, 1, 4, 5]
    };
    let column_faces: Vec<_> = anchor_columns
        .iter()
        .map(|&id| {
            (
                ElemId(id),
                frame_faces(&record.src_frame_faces, ElemId(id)).unwrap()[0],
            )
        })
        .collect();
    for purpose in [EvaluationPurpose::Ds, EvaluationPurpose::HoldingCapacity] {
        let point = run
            .evaluation_point(purpose, direction, step, "照合用モデルの第1加力点".into())
            .unwrap_or_else(|reason| {
                let initial = &run.confirmed_history.as_ref().unwrap()[0];
                panic!(
                    "{direction:?} {sign:?}: {reason}\n初期端部 {:?}\n初期層力 {:?}",
                    initial.src_walls, initial.cuts
                )
            });
        match purpose {
            EvaluationPurpose::Ds => run.ds_evaluation = Some(point),
            EvaluationPurpose::HoldingCapacity => run.capacity_evaluation = Some(point),
        }
    }
    for purpose in [EvaluationPurpose::Ds, EvaluationPurpose::HoldingCapacity] {
        app.adopt_holding_evaluation_for(direction, sign, purpose)
            .unwrap();
    }
    app.prepare_src_holding_confirmation().unwrap();
    let confirmation = app
        .core
        .scoped
        .results
        .as_mut()
        .unwrap()
        .holding_evaluations
        .iter_mut()
        .find(|v| {
            v.point.direction == direction
                && v.point.purpose == EvaluationPurpose::Ds
                && result_validity::holding_run_sign(&v.run) == sign
        })
        .unwrap()
        .src_confirmation
        .as_mut()
        .unwrap();
    confirmation.reviewer = "試験用設計者".into();
    confirmation.collapse = Some(if direction == SeismicDir::X {
        SrcCollapse::Partial
    } else {
        SrcCollapse::Overall
    });
    confirmation.collapse_basis = "試験用崩壊形資料図1".into();
    for proof in confirmation.girders.values_mut() {
        proof.ds_limits = test_face_limits();
        proof.strength_basis = "梁端の材料強度と同時作用下耐力を記した照合用計算表".into();
    }
    if direction == SeismicDir::X {
        for end in [SrcMemberEnd::I, SrcMemberEnd::J] {
            let limit =
                &mut confirmation.girders.get_mut(&origin_id).unwrap().ds_limits[end.index()];
            set_bending_limit(limit, &origin_faces[end.index()]);
            confirmation.ds_origins.push(SrcCollapseOrigin {
                elem: Some(origin_id),
                end: Some(end),
                failure: Some(SrcFailure::Bending),
                basis: "壁と平行な剛節架構の梁端曲げ到達点照合用の強度設定".into(),
            });
        }
    }
    confirmation.other_cases_basis = "代替機構との比較表1".into();
    confirmation.other_cases = Some(SrcOtherCollapseCases::NoOtherCase);
    confirmation.additional_conditions_basis = "設計条件表1".into();
    confirmation.no_prior_frame_shear = Some(true);
    confirmation.all_physical_members_counted = Some(true);
    for layer in &mut confirmation.layers {
        layer.application_basis = "SRC主体の構造伏図・軸組図1".into();
        layer.layout = Some(if confirmation.walls.is_empty() {
            SrcFrameLayout::MomentFrame
        } else {
            SrcFrameLayout::MomentFrameWithWalls
        });
        layer.uses_collapse_step_for_ds = Some(true);
    }
    for column in confirmation.columns.values_mut() {
        column.failure = Some(SrcFailure::Bending);
        column.failure_reached = Some(false);
        column.failure_basis = "柱曲げ・埋込み柱脚確認図1".into();
        column.base = Some(SrcColumnBase::NoUnembeddedBaseYield);
        column.n0_kn = 8_000.0;
        column.steel_m0_knm = 400.0;
        column.m0_knm = 800.0;
        column.horizontal_capacity_kn = 100.0;
        column.strength_basis = "材料強度と断面の耐力表1".into();
        column.ds_limits = std::array::from_fn(|_| SrcFrameFaceLimits {
            axial_kn: 8000.0,
            bending_strong_knm: 200.0,
            bending_weak_knm: 200.0,
            shear_strong_kn: 1000.0,
            shear_weak_kn: 1000.0,
        });
    }
    for (base_id, face) in column_faces {
        let base = confirmation.columns.get_mut(&base_id).unwrap();
        base.failure_reached = Some(true);
        set_bending_limit(&mut base.ds_limits[0], &face);
        confirmation.ds_origins.push(SrcCollapseOrigin {
            elem: Some(base_id),
            end: Some(SrcMemberEnd::I),
            failure: Some(SrcFailure::Bending),
            basis: "最下評価層の各柱脚実端の曲げ到達点照合用の強度設定".into(),
        });
    }
    for wall in confirmation.walls.values_mut() {
        wall.failure = Some(SrcFailure::Other);
        wall.failure_basis = "梁両端を含む崩壊形、壁の曲げ・せん断未到達を確認した図1".into();
        wall.horizontal_capacity_kn = 100.0;
        wall.strength_basis = "壁の水平耐力計算表1".into();
    }
}

fn set_bending_limit(limit: &mut SrcFrameFaceLimits, face: &[f64; 6]) {
    let demand_nmm = face[4].abs().max(face[5].abs());
    assert!(demand_nmm > 1.0, "{face:?}");
    let capacity_knm = demand_nmm * 1.00000001 / 1e6;
    if face[4].abs() >= face[5].abs() {
        limit.bending_weak_knm = capacity_knm;
    } else {
        limit.bending_strong_knm = capacity_knm;
    }
}

#[test]
fn src_holding_persists_all_four_saved_cases_and_rejects_stale_and_mixed_models() {
    let mut app = fixture();
    for (dir, sign) in SRC_HOLDING_CASES.into_iter().take(3) {
        save_case(&mut app, dir, sign);
    }
    let missing = app
        .compute_holding_capacity()
        .err()
        .expect("Y−の根拠が不足");
    assert!(missing.contains("Y−"), "{missing}");
    assert!(app.core.scoped.holding_capacity_source.is_none());

    save_case(&mut app, SeismicDir::Y, PushoverSign::Negative);
    {
        let results = app.core.scoped.results.as_mut().unwrap();
        let ds = results
            .holding_evaluations
            .iter_mut()
            .find(|e| {
                e.point.direction == SeismicDir::Y
                    && e.point.purpose == EvaluationPurpose::Ds
                    && result_validity::holding_run_sign(&e.run) == PushoverSign::Negative
            })
            .unwrap();
        let proof = ds.src_confirmation.as_mut().unwrap();
        proof
            .columns
            .values_mut()
            .next()
            .unwrap()
            .failure_basis
            .clear();
    }
    let missing = app
        .compute_holding_capacity()
        .err()
        .expect("柱破壊形式の根拠が不足");
    assert!(missing.contains("曲げ・せん断破壊と確認資料"), "{missing}");
    assert!(app.core.scoped.holding_capacity_source.is_none());
    save_case(&mut app, SeismicDir::Y, PushoverSign::Negative);
    let verified = app.src_holding_results().unwrap();
    assert_eq!(verified.len(), 4);
    assert_eq!(verified[0].source.ds_forces[0].wall_n.signum(), 1.0);
    assert_eq!(verified[1].source.ds_forces[0].wall_n.signum(), -1.0);
    for case in &verified[..2] {
        let beta = case.source.ds_forces[0].beta_u;
        assert!(0.3 < beta && beta <= 0.7, "{beta}");
        assert_eq!(case.result.stories[0].ds, 0.30);
    }
    assert_eq!(verified[2].result.stories[0].ds, 0.25);
    assert_eq!(verified[3].result.stories[0].ds, 0.25);
    for case in &verified[2..] {
        let story = &case.result.stories[0];
        assert!((story.qud - 1_000_000.0).abs() < 1e-6);
        assert!((story.qun - 250_000.0 * story.fes).abs() < 1e-6);
        assert!(!story.ok);
    }
    assert!(verified.iter().all(|s| s.result.stories[0].qu > 0.0));

    let ds_x = app
        .core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .holding_evaluations
        .iter()
        .position(|e| {
            e.point.direction == SeismicDir::X
                && e.point.purpose == EvaluationPurpose::Ds
                && result_validity::holding_run_sign(&e.run) == PushoverSign::Positive
        })
        .unwrap();
    let original = app
        .core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .holding_evaluations[ds_x]
        .clone();
    let replace = |app: &mut App, mutate: fn(&mut SrcHoldingConfirmation)| {
        let mut trial = original.clone();
        mutate(trial.src_confirmation.as_mut().unwrap());
        app.core
            .scoped
            .results
            .as_mut()
            .unwrap()
            .holding_evaluations[ds_x] = trial;
        app.compute_holding_capacity()
            .err()
            .expect("矛盾した設計根拠は保留")
    };
    for (mutate, message) in [
        (
            (|p: &mut SrcHoldingConfirmation| {
                for limit in &mut p.girders.get_mut(&ElemId(6)).unwrap().ds_limits {
                    limit.bending_weak_knm = 2000.0;
                    limit.bending_strong_knm = 2000.0;
                }
            }) as fn(&mut SrcHoldingConfirmation),
            "崩壊形の構成部材として宣言した曲げ・せん断耐力に達していません",
        ),
        (
            |p: &mut SrcHoldingConfirmation| {
                p.collapse = Some(SrcCollapse::Overall);
                p.ds_origins.truncate(1);
            },
            "全体崩壊形には各評価層に対応する複数の確定部材端",
        ),
        (
            |p: &mut SrcHoldingConfirmation| {
                p.collapse = Some(SrcCollapse::Overall);
                p.ds_origins.truncate(2);
            },
            "全体崩壊形には最下評価層の柱脚または耐震壁のせん断破壊の記録",
        ),
        (
            |p: &mut SrcHoldingConfirmation| {
                p.girders
                    .get_mut(&ElemId(3))
                    .unwrap()
                    .strength_basis
                    .clear()
            },
            "SRC梁 ID 3: 各採用点の両端耐力と確認根拠",
        ),
        (
            |p: &mut SrcHoldingConfirmation| {
                p.other_cases = Some(SrcOtherCollapseCases::ReviewedMinimum);
                p.layers[0].other_cases_min_qu_kn = 1.0e-8;
            },
            "他の有効崩壊形で確認した最小Qu",
        ),
        (
            |p: &mut SrcHoldingConfirmation| {
                p.walls.values_mut().next().unwrap().failure = Some(SrcFailure::Shear)
            },
            "38条せん断・曲げ耐力への到達状況",
        ),
        (
            |p: &mut SrcHoldingConfirmation| {
                p.walls.values_mut().next().unwrap().failure = Some(SrcFailure::Bending)
            },
            "38条せん断・曲げ耐力への到達状況",
        ),
        (
            |p: &mut SrcHoldingConfirmation| {
                p.walls.values_mut().next().unwrap().horizontal_capacity_kn = 1.0e6
            },
            "38条せん断耐力の方向成分",
        ),
        (
            |p: &mut SrcHoldingConfirmation| {
                p.walls.values_mut().next().unwrap().horizontal_capacity_kn = 1.0e-8
            },
            "確定端の壁負担力を下回ります",
        ),
        (
            |p: &mut SrcHoldingConfirmation| {
                p.columns
                    .values_mut()
                    .find(|column| column.failure_reached == Some(false))
                    .unwrap()
                    .failure_reached = Some(true)
            },
            "確定断面力と同時作用下耐力に整合しません",
        ),
        (
            |p: &mut SrcHoldingConfirmation| {
                p.layers[0].layout = Some(SrcFrameLayout::WallStructure)
            },
            "柱・壁の群またはβuが適用表と一致しません",
        ),
    ] {
        let reason = replace(&mut app, mutate);
        assert!(reason.contains(message), "{reason}");
    }
    let mut excessive_moment = original.clone();
    excessive_moment
        .run
        .confirmed_history
        .as_mut()
        .unwrap()
        .iter_mut()
        .find(|r| r.step == excessive_moment.point.step)
        .unwrap()
        .src_walls[0]
        .ends
        .as_mut()
        .unwrap()[0]
        .moment_nmm = 1.0e12;
    app.core
        .scoped
        .results
        .as_mut()
        .unwrap()
        .holding_evaluations[ds_x] = excessive_moment;
    let reason = app
        .compute_holding_capacity()
        .err()
        .expect("38条曲げ耐力超過は保留");
    assert!(reason.contains("38条曲げ耐力を超過"), "{reason}");
    let mut fd_column = original.clone();
    let record = fd_column
        .run
        .confirmed_response(&fd_column.point, EvaluationPurpose::Ds)
        .unwrap();
    let confirmation = fd_column.src_confirmation.as_mut().unwrap();
    let (&column_id, column) = confirmation.columns.iter_mut().next().unwrap();
    let compression_n =
        column_compression_n(&frame_faces(&record.src_frame_faces, column_id).unwrap());
    assert!(compression_n > 0.0);
    column.n0_kn = 2.0 * compression_n / 1000.0;
    app.core
        .scoped
        .results
        .as_mut()
        .unwrap()
        .holding_evaluations[ds_x] = fd_column;
    let reason = app
        .compute_holding_capacity()
        .err()
        .expect("FD柱除去の局部崩壊確認が不足");
    assert!(reason.contains("FD柱除去による局部崩壊の有無"), "{reason}");
    let entry = &mut app
        .core
        .scoped
        .results
        .as_mut()
        .unwrap()
        .holding_evaluations[ds_x];
    let layer = &mut entry.src_confirmation.as_mut().unwrap().layers[0];
    layer.fd_removal = Some(true);
    layer.fd_removal_basis = "試験用のFD側柱除去時の局部崩壊図2".into();
    assert_eq!(
        app.src_holding_results().unwrap()[0].result.stories[0].ds,
        0.45
    );
    app.core
        .scoped
        .results
        .as_mut()
        .unwrap()
        .holding_evaluations[ds_x] = original;

    let path =
        std::env::temp_dir().join(format!("sepika-src-holding-{}.ovika", std::process::id()));
    app.save_project_to_opts(path.clone(), Some(false));
    let mut reopened = App::default();
    reopened.open_project_from(path.clone());
    std::fs::remove_file(path).unwrap();
    let saved = reopened.src_holding_results().unwrap();
    for (before, after) in verified.iter().zip(saved) {
        assert_eq!(before.source.ds_point.run_id, after.source.ds_point.run_id);
        assert_eq!(before.result.stories[0].qu, after.result.stories[0].qu);
        assert_eq!(before.result.stories[0].qun, after.result.stories[0].qun);
    }
    let report = crate::summary::build_report_csv(&reopened);
    assert!(report.contains("X±・Y±全階,NG"));
    assert!(report.contains("Y−,1,"));
    assert!(report.contains("[増分解析 X−]"));
    assert!(report.contains("[目的別採用run Ds X−]"));

    reopened.core.model.nodes[2].coord[2] += 1.0;
    assert!(reopened.compute_holding_capacity().is_err());
    assert!(reopened.core.scoped.holding_capacity_source.is_none());

    let mut mixed = fixture();
    let girder = &mut mixed.core.model.sections[1];
    let Some(SectionShape::SrcBeamRect { b, d, rebar, .. }) = girder.shape.take() else {
        panic!()
    };
    girder.shape = Some(SectionShape::RcBeamRect { b, d, rebar });
    let reason = mixed.compute_holding_capacity().err().expect("適用範囲外");
    assert!(reason.contains("周辺梁はSRC梁が必要"), "{reason}");
    mixed.core.design_frame = FrameType::SteelFrame;
    assert!(mixed
        .compute_holding_capacity()
        .err()
        .expect("他種用Dsでは未判定")
        .contains("SRC用"));
}
