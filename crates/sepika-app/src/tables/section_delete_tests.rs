use super::sections_table;
use crate::app::App;
use sepika_core::{
    ids::{SectionId, SlabId},
    model::{SecondaryMember, SecondaryMemberKind, Slab, SlabPlate, SlabShape, StbSectionStrength},
};
use sepika_edit::{DeleteSection, UndoStack};

fn app_with_reference(reference: &str, strength: bool) -> App {
    let mut app = App::default();
    let mut model = crate::sample::portal_frame();
    model.sections.truncate(1);
    model.elements.truncate(1);
    if reference != "element" {
        model.elements.clear();
    }
    match reference {
        "slab" => model.slabs.push(Slab {
            id: SlabId(0),
            shape: SlabShape::Enclosed,
            plate: SlabPlate {
                section: Some(SectionId(0)),
                ..Default::default()
            },
            reference_story: None,
            vertical_eccentricity_mm: None,
            tip_loads: vec![],
        }),
        "beam" | "post" => {
            let mut member = SecondaryMember {
                section: Some(SectionId(0)),
                ..Default::default()
            };
            if reference == "post" {
                member.kind = SecondaryMemberKind::Post;
                model.unassigned_posts.push(member);
            } else {
                model.unassigned_beams.push(member);
            }
        }
        "none" | "element" => {}
        _ => panic!("未知の参照: {reference}"),
    }
    if strength {
        model.stb_strengths.sections.push(StbSectionStrength {
            section: SectionId(0),
            concrete: Some("Fc30".into()),
            native_material: None,
            reinforcement: vec![],
            steel: vec![],
            built_box: None,
            native_rebar: false,
            rebar: None,
        });
    }
    app.core.model = model;
    app
}

fn draw(ctx: &egui::Context, app: &mut App, events: Vec<egui::Event>) -> egui::FullOutput {
    ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(3200., 1000.),
            )),
            events,
            ..Default::default()
        },
        |ui| sections_table(ui, app),
    )
}

fn texts(output: &egui::FullOutput) -> Vec<(String, egui::Rect)> {
    fn visit(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::Shape::Text(text) => {
                out.push((text.galley.text().into(), text.visual_bounding_rect()))
            }
            egui::Shape::Vec(parts) => parts.iter().for_each(|s| visit(s, out)),
            _ => {}
        }
    }
    let mut out = vec![];
    for shape in &output.shapes {
        visit(&shape.shape, &mut out);
    }
    out
}

fn click(ctx: &egui::Context, app: &mut App, pos: egui::Pos2) {
    for pressed in [true, false] {
        draw(
            ctx,
            app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}

#[test]
fn section_table_reference_counts_disabled_delete_and_hover_reasons_match_command() {
    for reference in ["none", "element", "slab", "beam", "post"] {
        for strength in [false, true] {
            if reference == "none" && !strength {
                continue;
            }
            let mut app = app_with_reference(reference, strength);
            let before = app.core.model.clone();
            let ctx = egui::Context::default();
            ctx.global_style_mut(|style| {
                style.interaction.tooltip_delay = 0.0;
                style.interaction.show_tooltips_only_when_still = false;
            });
            draw(&ctx, &mut app, vec![]);
            let output = draw(&ctx, &mut app, vec![]);
            let labels = texts(&output);
            let count_column = labels.iter().find(|(s, _)| s == "部材数").unwrap().1;
            let expected = if reference == "none" { "0" } else { "1" };
            assert!(
                labels.iter().any(|(s, rect)| s == expected
                    && (rect.center().x - count_column.center().x).abs() < 35.0),
                "部材数: {reference}, 強度={strength}"
            );
            let delete_pos = labels.iter().find(|(s, _)| s == "🗑").unwrap().1.center();
            let mut output = draw(&ctx, &mut app, vec![egui::Event::PointerMoved(delete_pos)]);
            for _ in 0..3 {
                output = draw(&ctx, &mut app, vec![]);
            }
            let labels = texts(&output);
            let reason = labels
                .iter()
                .find(|(s, _)| s.contains("参照中のため削除できません"))
                .unwrap();
            assert_eq!(reason.0.contains("ST-Bridge断面強度入力"), strength);
            assert_eq!(
                reason.0.contains("部材・小梁・二次部材"),
                reference != "none"
            );
            click(&ctx, &mut app, delete_pos);
            assert!(app.core.model.eq_ignoring_dofmap(&before));
            assert!(!app.core.scoped.undo.can_undo());
            let mut history = UndoStack::new();
            assert!(!history.run(
                &mut app.core.model,
                Box::new(DeleteSection { id: SectionId(0) })
            ));
            assert!(app.core.model.eq_ignoring_dofmap(&before));
        }
    }
}

#[test]
fn section_table_unreferenced_delete_click_supports_undo_and_redo() {
    let mut app = app_with_reference("none", false);
    let before = app.core.model.clone();
    let ctx = egui::Context::default();
    draw(&ctx, &mut app, vec![]);
    let output = draw(&ctx, &mut app, vec![]);
    let pos = texts(&output)
        .into_iter()
        .find(|(s, _)| s == "🗑")
        .unwrap()
        .1
        .center();
    click(&ctx, &mut app, pos);
    assert!(app.core.model.sections.is_empty());
    app.undo_action();
    assert!(app.core.model.eq_ignoring_dofmap(&before));
    app.redo_action();
    assert!(app.core.model.sections.is_empty());
}
