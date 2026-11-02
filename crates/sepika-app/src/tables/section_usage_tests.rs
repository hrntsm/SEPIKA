use super::sections_table;
use crate::app::App;
use sepika_core::{
    ids::{FloorRegionId, SecondaryMemberId, SectionId, SlabId, WallPlateId, WallRegionId},
    model::{
        FloorRegion, SecondaryMember, SecondaryMemberKind, Slab, SlabPlate, SlabShape, WallPlate,
        WallPlateShape, WallRegion,
    },
    section_shape::SectionShape,
};
use sepika_edit::DeleteSection;

fn draw(ctx: &egui::Context, app: &mut App, events: Vec<egui::Event>) -> egui::FullOutput {
    ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(3200.0, 1000.0),
            )),
            events,
            ..Default::default()
        },
        |ui| sections_table(ui, app),
    )
}

fn texts(output: &egui::FullOutput) -> Vec<&egui::epaint::TextShape> {
    fn visit<'a>(shape: &'a egui::Shape, out: &mut Vec<&'a egui::epaint::TextShape>) {
        match shape {
            egui::Shape::Text(text) => out.push(text),
            egui::Shape::Vec(parts) => parts.iter().for_each(|part| visit(part, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
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
fn section_usage_table_counts_each_reference_and_only_deletes_unused_sections() {
    for usage in [
        "wall_plate",
        "element",
        "slab",
        "region_beam",
        "region_post",
        "unassigned_beam",
        "unassigned_post",
    ] {
        let mut model = crate::sample::portal_frame();
        model.sections = ["使用中", "未使用"]
            .into_iter()
            .enumerate()
            .map(|(i, name)| {
                SectionShape::RcSlab { thickness: 150.0 }
                    .to_section(SectionId(i as u32), name.into())
            })
            .collect();
        let mut element = model.elements[0].clone();
        model.elements.clear();
        let mut floor_region = FloorRegion::new(FloorRegionId(0), Vec::new());
        let mut wall_region = WallRegion::new(WallRegionId(0), Vec::new());
        for i in 0..2 {
            let member = SecondaryMember {
                id: SecondaryMemberId(i),
                kind: if usage.ends_with("post") {
                    SecondaryMemberKind::Post
                } else {
                    SecondaryMemberKind::Beam
                },
                section: Some(SectionId(0)),
                ..Default::default()
            };
            match usage {
                "wall_plate" => model.wall_plates.push(WallPlate {
                    id: WallPlateId(i),
                    shape: WallPlateShape::Enclosed,
                    section: Some(SectionId(0)),
                    opening_area: 0.0,
                    opening_weight: 0.0,
                    openings: Vec::new(),
                    loads: Vec::new(),
                    slit: Default::default(),
                    dl_support: None,
                    self_weight_shares: Vec::new(),
                }),
                "element" => {
                    element.id = sepika_core::ids::ElemId(i);
                    element.section = Some(SectionId(0));
                    model.elements.push(element.clone());
                }
                "slab" => model.slabs.push(Slab {
                    id: SlabId(i),
                    shape: SlabShape::Enclosed,
                    plate: SlabPlate {
                        section: Some(SectionId(0)),
                        ..Default::default()
                    },
                    reference_story: None,
                    vertical_eccentricity_mm: None,
                    tip_loads: Vec::new(),
                }),
                "region_beam" => floor_region.secondary_beams.push(member),
                "region_post" => wall_region.posts.push(member),
                "unassigned_beam" => model.unassigned_beams.push(member),
                "unassigned_post" => model.unassigned_posts.push(member),
                _ => unreachable!(),
            }
        }
        model.floor_regions.push(floor_region);
        model.wall_regions.push(wall_region);
        let mut app = App::default();
        app.load_model(model);
        let ctx = egui::Context::default();
        draw(&ctx, &mut app, Vec::new());
        let output = draw(&ctx, &mut app, Vec::new());
        let text = texts(&output);
        let header = text.iter().find(|t| t.galley.text() == "部材数").unwrap();
        for (name, count) in [("使用中", "2"), ("未使用", "0")] {
            let row = text.iter().find(|t| t.galley.text() == name).unwrap();
            assert!(
                text.iter().any(|t| {
                    t.galley.text() == count
                        && (t.pos.x - header.pos.x).abs() < 1.0
                        && (t.pos.y - row.pos.y).abs() < 1.0
                }),
                "{usage}: {name}の部材数は{count}"
            );
        }
        let buttons: Vec<_> = text.iter().filter(|t| t.galley.text() == "🗑").collect();
        assert_eq!(buttons.len(), 2, "{usage}");
        assert_ne!(
            buttons[0].fallback_color, buttons[1].fallback_color,
            "{usage}: 使用中の削除ボタンは無効表示"
        );
        let used_pos = buttons[0].pos + buttons[0].galley.size() * 0.5;
        let unused_pos = buttons[1].pos + buttons[1].galley.size() * 0.5;
        click(&ctx, &mut app, used_pos);
        assert_eq!(
            app.core.model.sections.len(),
            2,
            "{usage}: 使用中は削除不可"
        );
        app.apply_model_edit(Box::new(DeleteSection { id: SectionId(0) }));
        assert_eq!(
            app.core.model.sections.len(),
            2,
            "{usage}: コマンドも削除拒否"
        );
        click(&ctx, &mut app, unused_pos);
        assert_eq!(
            app.core.model.sections.len(),
            1,
            "{usage}: 未使用はクリックで削除"
        );
        assert_eq!(app.core.model.sections[0].name, "使用中");
    }
}
