use super::*;
use sepika_core::ids::{ElemId, SecondaryMemberId, SectionId, SlabId};
use sepika_core::model::{
    HighStrengthBolt, PlateAssignment, SecondaryBeamBoltConnection, SecondaryMemberAnchor,
    SecondaryMemberEnds, SupportMemberId,
};

fn connection(count: u32) -> SecondaryBeamBoltConnection {
    SecondaryBeamBoltConnection {
        bolt: HighStrengthBolt::F8TGalvanized,
        diameter_mm: 20,
        friction_surfaces: 2,
        count,
        surface_treatment_confirmed: true,
        concentric_shear_only_confirmed: true,
    }
}

fn multiple_beams() -> App {
    let mut app = super::result_contract_tests::direct_secondary_beam_app();
    let model = &mut app.core.model;
    let mut second = model.floor_regions[0].secondary_beams[0].clone();
    second.id = SecondaryMemberId(1);
    second.name = "J2".into();
    let SecondaryMemberEnds::Supported(ref mut ends) = second.ends else {
        panic!()
    };
    ends[0].position = 0.25;
    ends[1].position = 0.75;
    model.floor_regions[0].secondary_beams.push(second);
    let mut third = model.floor_regions[0].secondary_beams[0].clone();
    third.id = SecondaryMemberId(2);
    third.name = "J3".into();
    third.ends = SecondaryMemberEnds::Supported([
        SecondaryMemberAnchor {
            support: SupportMemberId::Primary(ElemId(1)),
            position: 0.25,
        },
        SecondaryMemberAnchor {
            support: SupportMemberId::Secondary(SecondaryMemberId(0)),
            position: 0.25,
        },
    ]);
    model.floor_regions[0].secondary_beams.push(third);
    model.next_secondary_member_id = 3;
    let slab = model.slabs[0].clone();
    model.slabs.clear();
    model.floor_assignment_regions = Default::default();
    model.rebuild_floor_assignment_regions();
    model.floor_regions[0].slab_ids.clear();
    for (index, region) in model
        .floor_assignment_regions
        .regions
        .iter_mut()
        .enumerate()
    {
        let mut s = slab.clone();
        s.id = SlabId(index as u32);
        for load in &mut s.plate.loads {
            load.value *= (index + 1) as f64;
        }
        region.assignment = PlateAssignment::Plate(s.id);
        model.floor_regions[0].slab_ids.push(s.id);
        model.slabs.push(s);
    }
    model.validate().unwrap();
    app
}

#[test]
fn symbol_counts_apply_to_all_beams_and_follow_assignment_changes() {
    let mut app = multiple_beams();
    let symbol = app.core.model.sections[0].name.clone();
    app.core
        .model
        .secondary_beam_bolts
        .insert(symbol.clone(), connection(2));
    let checks = app.floor_design_checks().unwrap().0;
    assert_eq!(checks.len(), 3);
    let first = checks[0].2.bolts.as_ref().unwrap();
    let second = checks[1].2.bolts.as_ref().unwrap();
    assert_ne!(first.q_n, second.q_n);
    assert_ne!(first.q_n[0], first.q_n[1]);
    for end in 0..2 {
        assert_eq!(first.ratios[end], Some(first.q_n[end].abs() / 141_600.0));
    }
    for (_, _, r) in &checks {
        let b = r.bolts.as_ref().unwrap();
        assert!(b.complete());
        assert_eq!(b.qa_n, Some(141_600.0));
    }
    app.core.scoped.undo.run(
        &mut app.core.model,
        Box::new(sepika_edit::SetSecondaryBeamBolts {
            symbol: symbol.clone(),
            input: Some(connection(4)),
        }),
    );
    let changed = app.floor_design_checks().unwrap().0;
    for ((_, _, before), (_, _, after)) in checks.iter().zip(&changed) {
        let before = before.bolts.as_ref().unwrap();
        let after = after.bolts.as_ref().unwrap();
        assert_eq!(after.qa_n, Some(283_200.0));
        assert_eq!(after.max_ratio(), before.max_ratio().map(|r| r / 2.0));
    }
    let mut other = app.core.model.sections[0].clone();
    other.id = SectionId(app.core.model.sections.len() as u32);
    other.name = "b2".into();
    let id = other.id;
    app.core.model.sections.push(other);
    app.core
        .model
        .secondary_beam_bolts
        .insert("b2".into(), connection(1));
    app.core.model.floor_regions[0].secondary_beams[1].section = Some(id);
    let checks = app.floor_design_checks().unwrap().0;
    assert_eq!(checks[0].2.bolts.as_ref().unwrap().qa_n, Some(283_200.0));
    assert_eq!(checks[1].2.bolts.as_ref().unwrap().qa_n, Some(70_800.0));
    app.core.model.sections[id.index()].name = symbol;
    app.core.model.sections[id.index()].floor = Some("2F".into());
    assert_eq!(
        app.floor_design_checks().unwrap().0[1]
            .2
            .bolts
            .as_ref()
            .unwrap()
            .qa_n,
        Some(283_200.0)
    );
}

#[test]
fn unset_connection_retains_bending_and_deflection_without_overall_pass() {
    let app = multiple_beams();
    for (_, _, r) in app.floor_design_checks().unwrap().0 {
        assert!(r.unchecked);
        assert!(!r.ok);
        assert!(r.bending_ratio > 0.0);
        assert!(r.deflection_ratio > 0.0);
        let bolts = r.bolts.unwrap();
        assert!(!bolts.complete());
        assert_eq!(bolts.max_ratio(), None);
        assert!(bolts.reasons[0].contains("未設定"));
    }
}

#[test]
fn bolt_conditions_and_results_roundtrip_and_edit_invalidates() {
    let mut app = super::result_contract_tests::direct_secondary_beam_app();
    let symbol = app.core.model.sections[0].name.clone();
    app.core
        .model
        .secondary_beam_bolts
        .insert(symbol.clone(), connection(3));
    app.run_design_check();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let (mut loaded, path) = super::result_contract_tests::restore(
        &app,
        None,
        Some(sepika_core::result_contract::DESIGN_CONTRACT_VERSION),
        "1048-bolts",
    );
    assert_eq!(
        loaded.core.model.secondary_beam_bolts,
        app.core.model.secondary_beam_bolts
    );
    assert_eq!(
        loaded.current_design_results().unwrap().beam_checks,
        app.current_design_results().unwrap().beam_checks
    );
    loaded.core.scoped.undo.run(
        &mut loaded.core.model,
        Box::new(sepika_edit::SetSecondaryBeamBolts {
            symbol,
            input: Some(connection(4)),
        }),
    );
    loaded.core.scoped.staleness.mark_edited();
    assert!(loaded.current_design_results().is_none());
    loaded.run_design_check();
    assert_eq!(
        loaded.current_design_results().unwrap().beam_checks[0]
            .2
            .bolts
            .as_ref()
            .unwrap()
            .qa_n,
        Some(283_200.0)
    );
    std::fs::remove_file(path).unwrap();
}

#[cfg(feature = "gui")]
#[test]
fn production_bolt_input_and_recheck_pointer_events_show_each_end_and_unchecked_reason() {
    let mut app = super::result_contract_tests::direct_secondary_beam_app();
    app.ui.scoped.nav.focus_section = Some(SectionId(0));
    app.run_design_check();
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
                    crate::section_editor::secondary_beam_bolt_panel(ui, app);
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
                    if (exact && t.galley.job.text == text)
                        || (!exact && t.galley.job.text.contains(text)) =>
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
                        modifiers: Default::default(),
                    },
                ],
            );
        }
    }
    let out = draw(&ctx, &mut app, false, vec![]);
    pos(&out, "断面符号のボルト接合条件が未設定");
    let out = draw(&ctx, &mut app, true, vec![]);
    click(&ctx, &mut app, true, pos(&out, "小梁端部の高力ボルト"));
    let out = draw(&ctx, &mut app, true, vec![]);
    click(&ctx, &mut app, true, pos(&out, "接合条件を設定"));
    let out = draw(&ctx, &mut app, true, vec![]);
    click(&ctx, &mut app, true, pos(&out, "F10T"));
    let out = draw(&ctx, &mut app, true, vec![]);
    click(&ctx, &mut app, true, pos(&out, "F8T（溶融亜鉛めっき）"));
    let out = draw(&ctx, &mut app, true, vec![]);
    click(&ctx, &mut app, true, pos(&out, "1面摩擦"));
    let out = draw(&ctx, &mut app, true, vec![]);
    click(&ctx, &mut app, true, pos(&out, "2面摩擦"));
    let out = draw(&ctx, &mut app, true, vec![]);
    click(&ctx, &mut app, true, pos(&out, "上記の摩擦面処理"));
    let out = draw(&ctx, &mut app, true, vec![]);
    click(&ctx, &mut app, true, pos(&out, "外力によるボルト引張なし"));
    let out = draw(&ctx, &mut app, true, vec![]);
    let p = pos(&out, "0");
    draw(
        &ctx,
        &mut app,
        true,
        vec![
            egui::Event::PointerMoved(p),
            egui::Event::PointerButton {
                pos: p,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
    );
    let end = p + egui::vec2(8.0, 0.0);
    draw(&ctx, &mut app, true, vec![egui::Event::PointerMoved(end)]);
    draw(
        &ctx,
        &mut app,
        true,
        vec![egui::Event::PointerButton {
            pos: end,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
    );
    let symbol = app.core.model.sections[0].name.clone();
    let i = &app.core.model.secondary_beam_bolts[&symbol];
    assert_eq!(i.bolt, HighStrengthBolt::F8TGalvanized);
    assert_eq!(i.friction_surfaces, 2);
    assert!(i.count > 0, "{i:?}");
    assert!(i.surface_treatment_confirmed && i.concentric_shear_only_confirmed);
    assert!(app.current_design_results().is_none());
    draw(&ctx, &mut app, false, vec![]);
    let out = draw(&ctx, &mut app, false, vec![]);
    click(&ctx, &mut app, false, pos(&out, "検定を再実行"));
    assert!(
        app.current_design_results().is_some(),
        "{:?}",
        app.core.scoped.last_error
    );
    let out = draw(&ctx, &mut app, false, vec![]);
    for text in [
        "始端 Q=",
        "終端 Q=",
        "F8T（溶融亜鉛めっき） M20・2面摩擦",
        "曲げ比",
        "せん断伝達OK",
        "接合部全体の安全性を示しません",
    ] {
        pos(&out, text);
    }
    let out = draw(&ctx, &mut app, true, vec![]);
    click(&ctx, &mut app, true, pos(&out, "F8T（溶融亜鉛めっき）"));
    let out = draw(&ctx, &mut app, true, vec![]);
    click(&ctx, &mut app, true, pos(&out, "F10T"));
    assert!(!app.core.model.secondary_beam_bolts[&symbol].surface_treatment_confirmed);
}
