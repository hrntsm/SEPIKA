use super::*;
use sepika_core::model::PostLateralSupport;
#[cfg(feature = "gui")]
use sepika_core::model::SecondaryMemberKind;
mod fixture {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../sepika-core/tests/support/steel_secondary_post_model.rs"
    ));
}
fn calculated() -> App {
    let mut app = App::default();
    let mut model = fixture::steel_post_model();
    let dead = model.post_design_inputs[0].lateral_loads[0].case;
    model.combinations[0].terms = vec![(dead, f64::MAX), (dead, 1.0), (dead, -f64::MAX)];
    app.load_model(model);
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    app.run_design_check();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let post = app
        .current_design_results()
        .expect("直接間柱検定")
        .beam_checks[0]
        .2
        .post
        .as_ref()
        .unwrap();
    assert!(!post.complete(), "{post:?}");
    assert!(post.combinations[0]
        .reasons
        .iter()
        .any(|r| r.contains("せん断力の断面形状・方向")));
    app
}

fn calculate_post_with_deflection_ratio_overflow(app: &mut App) {
    let model = &mut app.core.model;
    model.combinations[0].name = "たわみ限界超過の組合せ".into();
    model.materials[0].young = 205_000.0;
    let input = &mut model.post_design_inputs[0];
    input.lateral_support = PostLateralSupport::Pinned;
    input.deflection_limit_denom = Some(f64::MAX);
    input.lateral_loads[0].direction = [1.0, 0.0];
    input.lateral_loads[0].load = sepika_core::model::MemberLoadKind::Distributed {
        a: 0.0,
        b: 3000.0,
        w1: 2e5,
        w2: 2e5,
    };
    app.core.scoped.staleness.mark_edited();
    app.run_design_check();
}

#[test]
fn post_deflection_overflow_failure_roundtrips_without_a_numeric_ratio() {
    let mut app = calculated();
    calculate_post_with_deflection_ratio_overflow(&mut app);
    let version = sepika_core::result_contract::DESIGN_CONTRACT_VERSION;
    let (loaded, path) = super::result_contract_tests::restore(
        &app,
        None,
        Some(version),
        "1577-deflection-overflow",
    );
    let checks = &loaded.current_design_results().unwrap().beam_checks;
    assert_eq!(*checks, app.current_design_results().unwrap().beam_checks);
    let c = &checks[0].2.post.as_ref().unwrap().combinations[0];
    assert!(c.deflection_limit_exceeded_without_ratio);
    assert!(c.deflection_ratio.is_none());
    assert!(checks[0].2.deflection_span_ratio.is_finite());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn post_inputs_results_roundtrip_and_design_edit_invalidates() {
    let app = calculated();
    let version = sepika_core::result_contract::DESIGN_CONTRACT_VERSION;
    let (mut loaded, path) =
        super::result_contract_tests::restore(&app, None, Some(version), "1097-post");
    assert_eq!(
        loaded.core.model.post_design_inputs,
        app.core.model.post_design_inputs
    );
    assert_eq!(
        loaded.current_design_results().unwrap().beam_checks,
        app.current_design_results().unwrap().beam_checks
    );
    let mut changed = loaded.core.model.post_design_inputs[0].clone();
    changed.lateral_support = PostLateralSupport::FixedStart;
    assert!(loaded.core.scoped.undo.run(
        &mut loaded.core.model,
        Box::new(sepika_edit::SetPostDesignInput {
            member: changed.member,
            input: Some(changed)
        })
    ));
    loaded.core.scoped.staleness.mark_edited();
    assert!(loaded.current_design_results().is_none());
    loaded.run_design_check();
    assert!(!loaded.current_design_results().unwrap().beam_checks[0]
        .2
        .post
        .as_ref()
        .unwrap()
        .complete());
    let (old, old_path) =
        super::result_contract_tests::restore(&app, None, Some(version - 1), "1097-old");
    assert!(old.current_design_results().is_none());

    let normal_load = loaded.core.model.post_design_inputs[0].lateral_loads[0]
        .load
        .clone();
    loaded.core.model.post_design_inputs[0].lateral_loads[0].load =
        sepika_core::model::MemberLoadKind::Point {
            a: 3000.0,
            p: 1e306,
        };
    loaded.core.scoped.staleness.mark_edited();
    loaded.run_design_check();
    let (partial, partial_path) =
        super::result_contract_tests::restore(&loaded, None, Some(version), "1577-partial-post");
    let saved_checks = &partial.current_design_results().unwrap().beam_checks;
    assert_eq!(
        *saved_checks,
        loaded.current_design_results().unwrap().beam_checks
    );
    let combination = &saved_checks[0].2.post.as_ref().unwrap().combinations[0];
    assert!(combination.gravity_reactions.is_some());
    assert_eq!(
        combination.lateral_reactions,
        Some([[1e306, 0.0], [0.0, 0.0]])
    );
    assert!(combination.support_moments.is_none());
    let displacement = combination.displacement.as_ref().unwrap();
    assert_eq!(displacement.axes_mm, [None, Some(0.0)]);
    assert!(displacement.resultant_mm.is_none());

    loaded.core.model.materials[0].young = 2.05e-303;
    let input = &mut loaded.core.model.post_design_inputs[0];
    input.lateral_support = PostLateralSupport::Pinned;
    input.lateral_loads[0].load = normal_load;
    input.lateral_loads[0].direction = [0.01, 0.9999_f64.sqrt()];
    loaded.core.scoped.staleness.mark_edited();
    loaded.run_design_check();
    let (partial_displacement, displacement_path) = super::result_contract_tests::restore(
        &loaded,
        None,
        Some(version),
        "1577-partial-displacement",
    );
    let saved_checks = &partial_displacement
        .current_design_results()
        .unwrap()
        .beam_checks;
    assert_eq!(
        *saved_checks,
        loaded.current_design_results().unwrap().beam_checks
    );
    let combination = &saved_checks[0].2.post.as_ref().unwrap().combinations[0];
    let displacement = combination.displacement.as_ref().unwrap();
    assert!((displacement.axes_mm[0].unwrap() / 4.462086848252602e305 - 1.0).abs() < 1e-8);
    assert!(displacement.axes_mm[1].is_none());
    assert!(displacement.resultant_mm.is_none());
    assert!(saved_checks[0].2.deflection_span_ratio.is_finite());
    std::fs::remove_file(path).unwrap();
    std::fs::remove_file(old_path).unwrap();
    std::fs::remove_file(partial_path).unwrap();
    std::fs::remove_file(displacement_path).unwrap();
}
#[cfg(feature = "gui")]
#[test]
fn production_post_input_and_design_button_pointer_events() {
    let mut app = calculated();
    let normal_load = app.core.model.post_design_inputs[0].lateral_loads[0]
        .load
        .clone();
    let ctx = egui::Context::default();
    fn draw(
        ctx: &egui::Context,
        app: &mut App,
        input: bool,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(2600.0, 5000.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                if input {
                    crate::tables::secondary::secondary_member_list(
                        app,
                        ui,
                        SecondaryMemberKind::Post,
                    );
                } else {
                    crate::design_view::design_table(ui, app);
                }
            },
        )
    }
    fn pos(out: &egui::FullOutput, text: &str) -> egui::Pos2 {
        for exact in [true, false] {
            if let Some(p) = out.shapes.iter().find_map(|s| match &s.shape {
                egui::Shape::Text(t)
                    if if exact {
                        t.galley.job.text == text
                    } else {
                        t.galley.job.text.contains(text)
                    } =>
                {
                    Some(t.pos + t.galley.size() / 2.0)
                }
                _ => None,
            }) {
                return p;
            }
        }
        panic!("表示なし: {text}")
    }
    fn click(ctx: &egui::Context, app: &mut App, input: bool, p: egui::Pos2) {
        for pressed in [true, false] {
            draw(
                ctx,
                app,
                input,
                vec![
                    egui::Event::PointerMoved(p),
                    egui::Event::PointerButton {
                        pos: p,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    }
    let out = draw(&ctx, &mut app, true, vec![]);
    click(&ctx, &mut app, true, pos(&out, "SM0 検定入力"));
    let out = draw(&ctx, &mut app, true, vec![]);
    click(&ctx, &mut app, true, pos(&out, "始端固定・終端自由"));
    assert_eq!(
        app.core.model.post_design_inputs[0].lateral_support,
        PostLateralSupport::FixedStart
    );
    assert!(app.current_design_results().is_none());
    draw(&ctx, &mut app, false, vec![]);
    let out = draw(&ctx, &mut app, false, vec![]);
    click(&ctx, &mut app, false, pos(&out, "検定を再実行"));
    assert!(
        app.current_design_results().is_some(),
        "error={:?}, validity={:?}",
        app.core.scoped.last_error,
        app.require_design_input()
    );
    let out = draw(&ctx, &mut app, false, vec![]);
    let p = pos(&out, "間柱 間柱P1");
    click(&ctx, &mut app, false, p);
    let out = draw(&ctx, &mut app, false, vec![]);
    assert!(out.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.job.text.contains("自動転送しません"))));

    let model = &mut app.core.model;
    let dead = model.post_design_inputs[0].lateral_loads[0].case;
    model.combinations[0].name = "係数を算定できない組合せ".into();
    model.combinations[0].terms = vec![(dead, f64::MAX); 2];
    app.core.scoped.staleness.mark_edited();
    app.run_design_check();
    let out = draw(&ctx, &mut app, false, vec![]);
    pos(&out, "係数を算定できない組合せ");
    pos(&out, "荷重組合せ係数の合計が非有限です");
    assert_eq!(
        out.shapes
            .iter()
            .filter(|s| matches!(&s.shape, egui::Shape::Text(t)
        if t.galley.job.text == "未算定"))
            .count(),
        3
    );
    assert!(!out.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t)
        if t.galley.job.text.contains("鉛直端部反力") || t.galley.job.text.contains("同位置合成最大"))));

    let model = &mut app.core.model;
    model.combinations[0].name = "等厚角形鋼管の組合せ".into();
    model.combinations[0].terms = vec![(dead, 1.0)];
    model.materials[0].young = 2.05e155;
    model.post_design_inputs[0].lateral_loads[0].load =
        sepika_core::model::MemberLoadKind::Distributed {
            a: 0.0,
            b: 3000.0,
            w1: 1e155,
            w2: 1e155,
        };
    app.core.scoped.staleness.mark_edited();
    app.run_design_check();
    draw(&ctx, &mut app, false, vec![]);
    let out = draw(&ctx, &mut app, false, vec![]);
    pos(&out, "等厚角形鋼管の組合せ");
    pos(&out, "鉛直端部反力");
    pos(&out, "同位置合成最大");

    let model = &mut app.core.model;
    model.combinations[0].name = "横荷重応答を算定できない組合せ".into();
    model.post_design_inputs[0].lateral_loads[0].load =
        sepika_core::model::MemberLoadKind::Distributed {
            a: 0.0,
            b: 3000.0,
            w1: 1e305,
            w2: 1e305,
        };
    app.core.scoped.staleness.mark_edited();
    app.run_design_check();
    draw(&ctx, &mut app, false, vec![]);
    let out = draw(&ctx, &mut app, false, vec![]);
    pos(&out, "横荷重応答を算定できない組合せ");
    pos(&out, "鉛直端部反力 始 1.070 / 終 0.000 kN");
    assert!(!out
        .shapes
        .iter()
        .any(|s| matches!(&s.shape, egui::Shape::Text(t)
        if t.galley.job.text.contains("横反力 X"))));
    pos(&out, "同位置合成最大 未算定");

    let model = &mut app.core.model;
    model.combinations[0].name = "固定端曲げを算定できない組合せ".into();
    model.post_design_inputs[0].lateral_loads[0].load = sepika_core::model::MemberLoadKind::Point {
        a: 3000.0,
        p: 1e306,
    };
    app.core.scoped.staleness.mark_edited();
    app.run_design_check();
    draw(&ctx, &mut app, false, vec![]);
    let out = draw(&ctx, &mut app, false, vec![]);
    pos(&out, "固定端曲げを算定できない組合せ");
    pos(&out, "固定端曲げの計算で非有限値が発生しました");
    pos(&out, "横反力 X");
    pos(&out, "固定端曲げ 未算定");

    let model = &mut app.core.model;
    model.combinations[0].name = "弱軸方向の変位を算定できない組合せ".into();
    model.materials[0].young = 2.05e-303;
    let input = &mut model.post_design_inputs[0];
    input.lateral_support = PostLateralSupport::Pinned;
    input.lateral_loads[0].load = normal_load;
    input.lateral_loads[0].direction = [0.01, 0.9999_f64.sqrt()];
    app.core.scoped.staleness.mark_edited();
    app.run_design_check();
    draw(&ctx, &mut app, false, vec![]);
    let out = draw(&ctx, &mut app, false, vec![]);
    pos(&out, "強軸方向 4.4621e305 mm");
    pos(&out, "弱軸方向 未算定");
    pos(&out, "同位置合成最大 未算定");

    calculate_post_with_deflection_ratio_overflow(&mut app);
    draw(&ctx, &mut app, false, vec![]);
    let out = draw(&ctx, &mut app, false, vec![]);
    pos(&out, "たわみ限界超過の組合せ");
    pos(&out, "たわみ検定比の乗算で非有限値が発生しました");
    pos(&out, "たわみ検定比 NG（限界超過、比は未算定）");
}

#[cfg(feature = "gui")]
#[test]
fn section_editor_shows_missing_corner_radius_before_commit() {
    let mut app = App::default();
    app.ui.scoped.section_draft.kind = crate::section_editor::ShapeKind::SteelBox;
    let ctx = egui::Context::default();
    let before = app.core.model.clone();
    let revision = app.core.scoped.undo.revision();
    let out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(2600.0, 4000.0),
            )),
            ..Default::default()
        },
        |ui| crate::section_editor::section_editor_panel(ui, &mut app),
    );
    assert!(out.shapes.iter().any(
        |s| matches!(&s.shape,egui::Shape::Text(t) if t.galley.job.text.contains("角部外半径"))
    ));
    assert!(app.core.model.eq_ignoring_dofmap(&before));
    assert_eq!(app.core.scoped.undo.revision(), revision);
}
