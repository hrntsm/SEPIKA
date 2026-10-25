use super::{material_cell, role_applies, role_material, sections_table};
use crate::app::App;
use sepika_core::{
    ids::{MaterialId, SectionId},
    model::{MaterialCategory, Section},
    section_shape::SectionShape,
};
use sepika_edit::SectionMaterialRole;

fn shapes() -> [SectionShape; 2] {
    [
        SectionShape::CftBox {
            height: 400.,
            width: 400.,
            thick: 12.,
            horizontal_thick: 12.,
            corner_r: Some(0.),
        },
        SectionShape::CftPipe {
            outer_dia: 400.,
            thick: 12.,
        },
    ]
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

fn text_positions(output: &egui::FullOutput, label: &str) -> Vec<egui::Pos2> {
    fn visit(shape: &egui::Shape, label: &str, out: &mut Vec<egui::Pos2>) {
        match shape {
            egui::Shape::Text(text) if text.galley.text() == label => {
                out.push(text.pos + text.galley.size() * 0.5)
            }
            egui::Shape::Vec(parts) => parts.iter().for_each(|s| visit(s, label, out)),
            _ => {}
        }
    }
    let mut out = vec![];
    for shape in &output.shapes {
        visit(&shape.shape, label, &mut out);
    }
    out
}

fn click(ctx: &egui::Context, app: &mut App, label: &str, last: bool) {
    let output = draw(ctx, app, vec![]);
    let positions = text_positions(&output, label);
    let pos = *if last {
        positions.last()
    } else {
        positions.first()
    }
    .unwrap_or_else(|| panic!("表示なし: {label}"));
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
fn cft766_table_assign_clear_and_undo_redo_both_shapes() {
    for shape in shapes() {
        let mut model = crate::sample::portal_frame();
        let mut concrete = model.materials[0].clone();
        concrete.id = MaterialId(1);
        concrete.name = "充填専用".into();
        concrete.category = MaterialCategory::Concrete;
        model.materials[0].name = "鋼管専用".into();
        model.materials.push(concrete);
        model.sections = vec![Section {
            material: Some(MaterialId(1)),
            steel_material: Some(MaterialId(0)),
            frame_use: Some(sepika_core::model::FrameSectionUse::Column),
            ..shape.to_section(SectionId(0), "CFT".into())
        }];
        model.elements.truncate(1);
        let mut app = App::default();
        app.load_model(model);
        let ctx = egui::Context::default();
        assert!(role_applies(Some(&shape), SectionMaterialRole::Steel));
        assert!(!role_applies(Some(&shape), SectionMaterialRole::Rebar));
        assert!(!role_applies(Some(&shape), SectionMaterialRole::ShearRebar));
        let output = draw(&ctx, &mut app, vec![]);
        assert_eq!(text_positions(&output, "鋼管専用").len(), 1);
        click(&ctx, &mut app, "鋼管専用", false);
        let output = draw(&ctx, &mut app, vec![]);
        assert_eq!(
            text_positions(&output, "充填専用").len(),
            1,
            "充填材料は主材料セルだけに表示される"
        );
        click(&ctx, &mut app, "―", true);
        assert_eq!(app.core.model.sections[0].steel_material, None);
        app.undo_action();
        assert_eq!(
            app.core.model.sections[0].steel_material,
            Some(MaterialId(0))
        );
        app.redo_action();
        assert_eq!(app.core.model.sections[0].steel_material, None);
        click(&ctx, &mut app, "―", true);
        click(&ctx, &mut app, "鋼管専用", true);
        assert_eq!(
            role_material(&app.core.model.sections[0], SectionMaterialRole::Steel),
            Some(MaterialId(0))
        );
        assert_eq!(app.core.model.sections[0].material, Some(MaterialId(1)));
    }
}

#[test]
fn cft766_material_cell_shows_unassigned_instead_of_non_applicable() {
    let mut applicable = vec![];
    for shape in shapes() {
        let section = shape.to_section(SectionId(0), "CFT".into());
        let mut pending = vec![];
        let output = egui::Context::default().run_ui(Default::default(), |ui| {
            material_cell(
                ui,
                &Default::default(),
                &section,
                SectionMaterialRole::Steel,
                "baseline",
                &mut pending,
            );
        });
        applicable.push(
            text_positions(&output, "—").is_empty() && !text_positions(&output, "―").is_empty(),
        );
    }
    assert_eq!(
        applicable,
        [true, true],
        "角形・円形のCFT鋼管欄は未割当を表示する"
    );
}
