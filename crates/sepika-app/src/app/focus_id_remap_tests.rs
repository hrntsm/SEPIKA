use super::*;
use sepika_edit::{CompositeCommand, DeleteMaterial, DeleteSection, IdChange};

fn app_with_abc() -> App {
    let mut app = App::default();
    let seed = crate::sample::portal_frame();
    app.core.model = sepika_core::model::Model::default();
    for (index, name) in ["A", "B", "C"].into_iter().enumerate() {
        let mut material = seed.materials[0].clone();
        material.id = MaterialId(index as u32);
        material.name = name.into();
        app.core.model.materials.push(material);
        let mut section = seed.sections[0].clone();
        section.id = SectionId(index as u32);
        section.name = name.into();
        section.floor = None;
        section.material = None;
        section.rebar_material = None;
        section.shear_rebar_material = None;
        section.steel_material = None;
        app.core.model.sections.push(section);
    }
    app
}

#[test]
fn 下位削除とundo_redoは同じ材料と断面を開く() {
    let mut app = app_with_abc();
    app.ui.scoped.nav.focus_material = Some(MaterialId(2));
    app.ui.scoped.nav.focus_section = Some(SectionId(2));
    assert!(app.apply_model_edit(Box::new(DeleteMaterial { id: MaterialId(0) })));
    assert_eq!(app.ui.scoped.nav.focus_material, Some(MaterialId(1)));
    assert_eq!(app.ui.scoped.nav.focus_section, Some(SectionId(2)));
    assert_eq!(app.core.model.materials[1].name, "C");
    app.undo_action();
    assert_eq!(app.ui.scoped.nav.focus_material, Some(MaterialId(2)));
    app.redo_action();
    assert_eq!(app.ui.scoped.nav.focus_material, Some(MaterialId(1)));
    assert!(app.apply_model_edit(Box::new(DeleteSection { id: SectionId(0) })));
    assert_eq!(app.ui.scoped.nav.focus_section, Some(SectionId(1)));
    assert_eq!(app.ui.scoped.nav.focus_material, Some(MaterialId(1)));
    assert_eq!(app.core.model.sections[1].name, "C");
    app.undo_action();
    assert_eq!(app.ui.scoped.nav.focus_section, Some(SectionId(2)));
    app.redo_action();
    assert_eq!(app.ui.scoped.nav.focus_section, Some(SectionId(1)));
}

#[test]
fn 自身削除で解除した対象はundoでも自動で開かない() {
    let mut app = app_with_abc();
    app.ui.scoped.nav.focus_material = Some(MaterialId(1));
    app.ui.scoped.nav.focus_section = Some(SectionId(1));
    assert!(app.apply_model_edit(Box::new(DeleteMaterial { id: MaterialId(1) })));
    assert_eq!(app.ui.scoped.nav.focus_material, None);
    app.undo_action();
    assert_eq!(app.ui.scoped.nav.focus_material, None);
    assert_eq!(app.core.model.materials[1].name, "B");
    assert!(app.apply_model_edit(Box::new(DeleteSection { id: SectionId(1) })));
    assert_eq!(app.ui.scoped.nav.focus_section, None);
    app.undo_action();
    assert_eq!(app.ui.scoped.nav.focus_section, None);
    assert_eq!(app.core.model.sections[1].name, "B");
}

#[test]
fn 削除後に開いた別の実体はundoで維持する() {
    let mut app = app_with_abc();
    assert!(app.apply_model_edit(Box::new(DeleteMaterial { id: MaterialId(0) })));
    app.ui.scoped.nav.focus_material = Some(MaterialId(0));
    app.undo_action();
    assert_eq!(app.ui.scoped.nav.focus_material, Some(MaterialId(1)));
    assert_eq!(app.core.model.materials[1].name, "B");
    assert!(app.apply_model_edit(Box::new(DeleteSection { id: SectionId(0) })));
    app.ui.scoped.nav.focus_section = Some(SectionId(1));
    app.undo_action();
    assert_eq!(app.ui.scoped.nav.focus_section, Some(SectionId(2)));
    assert_eq!(app.core.model.sections[2].name, "C");
}

#[test]
fn 上位削除と参照中拒否とnoopは開いた対象を維持する() {
    let mut app = app_with_abc();
    app.ui.scoped.nav.focus_material = Some(MaterialId(0));
    app.ui.scoped.nav.focus_section = Some(SectionId(0));
    assert!(app.apply_model_edit(Box::new(DeleteMaterial { id: MaterialId(2) })));
    assert!(app.apply_model_edit(Box::new(DeleteSection { id: SectionId(2) })));
    app.core.model.sections[0].material = Some(MaterialId(0));
    app.core.model.elements = crate::sample::portal_frame().elements;
    app.core.scoped.staleness.results_stale = false;
    let revision = app.core.scoped.undo.revision();
    for command in [
        Box::new(DeleteMaterial { id: MaterialId(0) }) as Box<dyn sepika_edit::EditCommand>,
        Box::new(DeleteSection { id: SectionId(0) }),
        Box::new(DeleteMaterial { id: MaterialId(99) }),
        Box::new(DeleteSection { id: SectionId(99) }),
        Box::new(sepika_edit::Noop),
    ] {
        assert!(!app.apply_model_edit(command));
        assert!(app.core.scoped.undo.id_changes().is_empty());
        assert_eq!(app.ui.scoped.nav.focus_material, Some(MaterialId(0)));
        assert_eq!(app.ui.scoped.nav.focus_section, Some(SectionId(0)));
        assert_eq!(app.core.scoped.undo.revision(), revision);
        assert!(!app.core.scoped.staleness.results_stale);
    }
}

#[test]
fn 複数削除は同じ最終実体へ移り途中で対象を取り違えない() {
    let mut app = app_with_abc();
    app.ui.scoped.nav.focus_material = Some(MaterialId(2));
    app.ui.scoped.nav.focus_section = Some(SectionId(2));
    assert!(app.apply_model_edit(Box::new(CompositeCommand {
        label: "材料と断面をまとめて削除".into(),
        children: vec![
            Box::new(DeleteMaterial { id: MaterialId(1) }),
            Box::new(DeleteMaterial { id: MaterialId(99) }),
            Box::new(DeleteSection { id: SectionId(1) }),
            Box::new(DeleteMaterial { id: MaterialId(0) }),
            Box::new(DeleteSection { id: SectionId(0) }),
        ],
    })));
    assert_eq!(
        app.core.scoped.undo.id_changes(),
        &[
            IdChange::MaterialRemoved(MaterialId(1)),
            IdChange::SectionRemoved(SectionId(1)),
            IdChange::MaterialRemoved(MaterialId(0)),
            IdChange::SectionRemoved(SectionId(0)),
        ]
    );
    assert_eq!(app.ui.scoped.nav.focus_material, Some(MaterialId(0)));
    assert_eq!(app.ui.scoped.nav.focus_section, Some(SectionId(0)));
    assert_eq!(app.core.model.materials[0].name, "C");
    assert_eq!(app.core.model.sections[0].name, "C");
    app.undo_action();
    assert_eq!(app.ui.scoped.nav.focus_material, Some(MaterialId(2)));
    assert_eq!(app.ui.scoped.nav.focus_section, Some(SectionId(2)));
    app.redo_action();
    assert_eq!(app.ui.scoped.nav.focus_material, Some(MaterialId(0)));
    assert_eq!(app.ui.scoped.nav.focus_section, Some(SectionId(0)));
}

#[test]
fn 表の同フレームで開く対象も成功した削除に追従する() {
    let mut app = app_with_abc();
    crate::tables::materials::apply_pending_actions(&mut app, Some(MaterialId(2)), Some(0));
    crate::tables::sections::apply_pending_actions(
        &mut app,
        Some(SectionId(2)),
        Some(SectionId(0)),
    );
    assert_eq!(app.ui.scoped.nav.focus_material, Some(MaterialId(1)));
    assert_eq!(app.ui.scoped.nav.focus_section, Some(SectionId(1)));
    crate::tables::materials::apply_pending_actions(&mut app, Some(MaterialId(1)), Some(1));
    crate::tables::sections::apply_pending_actions(
        &mut app,
        Some(SectionId(1)),
        Some(SectionId(1)),
    );
    assert_eq!(app.ui.scoped.nav.focus_material, None);
    assert_eq!(app.ui.scoped.nav.focus_section, None);
    app.undo_action();
    app.undo_action();
    assert_eq!(app.ui.scoped.nav.focus_material, None);
    assert_eq!(app.ui.scoped.nav.focus_section, None);
}

#[test]
fn 複製した部材断面をundoで削除したとき開いた対象を解除する() {
    let mut app = App::default();
    app.core.model = crate::sample::portal_frame();
    let member = app.core.model.elements[0].id;
    assert!(app.apply_model_edit(Box::new(sepika_edit::DuplicateSectionForMember { member })));
    let duplicated = app.core.model.elements[0].section.unwrap();
    app.ui.scoped.nav.focus_section = Some(duplicated);
    app.undo_action();
    assert_eq!(app.ui.scoped.nav.focus_section, None);
    app.redo_action();
    assert_eq!(app.ui.scoped.nav.focus_section, None);
    assert!(app.core.model.section(duplicated).is_some());
}

fn table_frame(
    ctx: &egui::Context,
    app: &mut App,
    materials: bool,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(2000.0, 1000.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            if materials {
                crate::tables::materials::materials_table(ui, app);
            } else {
                crate::tables::sections::sections_table(ui, app);
            }
        },
    )
}

#[test]
fn 実際の材料断面表の削除ボタンで開いた実体を維持する() {
    for materials in [true, false] {
        let mut app = app_with_abc();
        app.ui.scoped.nav.focus_material = Some(MaterialId(2));
        app.ui.scoped.nav.focus_section = Some(SectionId(2));
        let ctx = egui::Context::default();
        table_frame(&ctx, &mut app, materials, vec![]);
        let output = table_frame(&ctx, &mut app, materials, vec![]);
        let pos = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) if text.galley.job.text == "🗑" => {
                    Some(text.pos + text.galley.size() / 2.0)
                }
                _ => None,
            })
            .min_by(|a, b| a.y.total_cmp(&b.y))
            .expect("先頭行の削除ボタン");
        for pressed in [true, false] {
            table_frame(
                &ctx,
                &mut app,
                materials,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
        if materials {
            assert_eq!(app.core.model.materials.len(), 2);
            assert_eq!(app.ui.scoped.nav.focus_material, Some(MaterialId(1)));
            assert_eq!(app.core.model.materials[1].name, "C");
        } else {
            assert_eq!(app.core.model.sections.len(), 2);
            assert_eq!(app.ui.scoped.nav.focus_section, Some(SectionId(1)));
            assert_eq!(app.core.model.sections[1].name, "C");
        }
        app.undo_action();
        assert_eq!(app.ui.scoped.nav.focus_material, Some(MaterialId(2)));
        assert_eq!(app.ui.scoped.nav.focus_section, Some(SectionId(2)));
    }
}

#[test]
fn ナビゲータで開いた材料断面は表削除後も同じ実体を表示する() {
    for materials in [true, false] {
        let mut app = app_with_abc();
        let ctx = egui::Context::default();
        let frame = |app: &mut App, events| {
            ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(600.0, 1000.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    if materials {
                        app.nav_materials(ui);
                    } else {
                        app.nav_sections(ui);
                    }
                },
            )
        };
        let labels = if materials {
            vec!["材料一覧", "鋼材", "[2] C"]
        } else {
            vec!["断面一覧", "（階なし）", "[2] C"]
        };
        for label in labels {
            frame(&mut app, vec![]);
            let output = frame(&mut app, vec![]);
            let pos = output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::epaint::Shape::Text(text) if text.galley.job.text == label => {
                        Some(text.pos + text.galley.size() / 2.0)
                    }
                    _ => None,
                })
                .expect(label);
            for pressed in [true, false] {
                frame(
                    &mut app,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: Default::default(),
                        },
                    ],
                );
            }
        }
        if materials {
            assert_eq!(app.ui.scoped.nav.focus_material, Some(MaterialId(2)));
            crate::tables::materials::apply_pending_actions(&mut app, None, Some(0));
            assert_eq!(app.ui.scoped.nav.focus_material, Some(MaterialId(1)));
            assert_eq!(app.core.model.materials[1].name, "C");
        } else {
            assert_eq!(app.ui.scoped.nav.focus_section, Some(SectionId(2)));
            crate::tables::sections::apply_pending_actions(&mut app, None, Some(SectionId(0)));
            assert_eq!(app.ui.scoped.nav.focus_section, Some(SectionId(1)));
            assert_eq!(app.core.model.sections[1].name, "C");
        }
    }
}

fn app_with_abc_and_generated_wall() -> App {
    use sepika_core::ids::{WallPlateId, WallRegionId};
    use sepika_core::model::{Node, WallPlate, WallPlateShape, WallRegion};
    use sepika_core::section_shape::SectionShape;
    let mut app = app_with_abc();
    for (index, coord) in [
        [0.0, 0.0, 0.0],
        [4000.0, 0.0, 0.0],
        [4000.0, 0.0, 3000.0],
        [0.0, 0.0, 3000.0],
    ]
    .into_iter()
    .enumerate()
    {
        app.core.model.nodes.push(Node {
            id: NodeId(index as u32),
            coord,
            restraint: Default::default(),
            mass: None,
            story: None,
            support_spring: None,
        });
    }
    app.core.model.sections[2] = SectionShape::RcWall {
        thickness: 180.0,
        pwh_ratio: None,
        ps: 0.0025,
    }
    .to_section(SectionId(2), "C".into());
    app.core.model.sections[2].material = Some(MaterialId(2));
    let boundary = (0..4).map(NodeId).collect::<Vec<_>>();
    app.core.model.add_enclosed_wall_plate_from_nodes(
        &boundary,
        WallPlate {
            id: WallPlateId(0),
            shape: WallPlateShape::Enclosed,
            section: Some(SectionId(2)),
            self_weight_shares: Vec::new(),
            opening_area: 0.0,
            opening_weight: 0.0,
            openings: Vec::new(),
            loads: Vec::new(),
            slit: Default::default(),
        },
    );
    app.core.model.wall_regions.push(WallRegion {
        id: WallRegionId(0),
        name: String::new(),
        boundary,
        wall_plate_ids: vec![WallPlateId(0)],
        posts: Vec::new(),
    });
    app.ui.scoped.nav.focus_material = Some(MaterialId(2));
    app.ui.scoped.nav.focus_section = Some(SectionId(2));
    app
}

fn check_preparation_keeps_focus(materials: bool, undo: bool) {
    let mut app = app_with_abc_and_generated_wall();
    if materials {
        assert!(app.apply_model_edit(Box::new(DeleteMaterial { id: MaterialId(0) })));
    } else {
        assert!(app.apply_model_edit(Box::new(DeleteSection { id: SectionId(0) })));
    }
    if undo {
        app.undo_action();
    }
    assert!(!app.core.scoped.undo.id_changes().is_empty());
    let material_focus = app.ui.scoped.nav.focus_material;
    let section_focus = app.ui.scoped.nav.focus_section;
    let (_, index, _) = sepika_load::wall_expand::expand_wall_elements(&app.core.model);
    let generated = index.generated_elem_ids().next().expect("実際の生成壁要素");
    assert!(app.core.model.element(generated).is_none());
    app.select_member(generated);
    assert_eq!(app.ui.scoped.selection.active_member(), Some(generated));
    app.ensure_preparation();
    assert_eq!(app.ui.scoped.selection, GeometrySelection::None);
    assert_eq!(app.ui.scoped.nav.focus_material, material_focus);
    assert_eq!(app.ui.scoped.nav.focus_section, section_focus);
    assert_eq!(
        app.core.model.materials[material_focus.unwrap().index()].name,
        "C"
    );
    assert_eq!(
        app.core.model.sections[section_focus.unwrap().index()].name,
        "C"
    );
}

#[test]
fn 準備計算の生成壁選択解除は材料断面の削除情報を再適用しない() {
    for materials in [true, false] {
        check_preparation_keeps_focus(materials, false);
    }
}

#[test]
fn 準備計算の生成壁選択解除はundo挿入後の材料断面を範囲外へ移さない() {
    for materials in [true, false] {
        check_preparation_keeps_focus(materials, true);
    }
}
