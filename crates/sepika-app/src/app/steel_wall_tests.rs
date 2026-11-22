use super::*;
use sepika_core::ids::{WallPlateId, WallRegionId};
use sepika_core::model::{PropertyBasis, WallPlate, WallPlateShape, WallRegion};

pub(super) fn steel_wall_model() -> sepika_core::model::Model {
    let mut model =
        crate::app::tests::with_explicit_pushover_gravity(crate::sample::portal_frame());
    model.panel_zone = sepika_core::model::PanelZoneMode::None;
    let mut bottom = model.elements[2].clone();
    bottom.id = ElemId(3);
    bottom.nodes = [NodeId(0), NodeId(1)].into_iter().collect();
    model.elements.push(bottom);
    let mut section = model.sections[1].clone();
    section.id = SectionId(2);
    section.name = "鋼板壁".into();
    section.frame_use = None;
    section.shape = None;
    section.property_basis.area = PropertyBasis::Supplied;
    section.property_basis.j = PropertyBasis::Supplied;
    section.thickness = Some(6.0);
    model.sections.push(section);
    let boundary = vec![NodeId(0), NodeId(1), NodeId(3), NodeId(2)];
    model.add_enclosed_wall_plate_from_nodes(
        &boundary,
        WallPlate {
            id: WallPlateId(0),
            shape: WallPlateShape::Enclosed,
            section: Some(SectionId(2)),
            dl_support: Some(sepika_core::model::WallDlSupport::LowerBeam),
            self_weight_shares: Vec::new(),
            opening_area: 0.0,
            opening_weight: 0.0,
            openings: Vec::new(),
            loads: Vec::new(),
            slit: Default::default(),
        },
    );
    model.wall_regions.push(WallRegion {
        id: WallRegionId(0),
        name: "鋼板壁構面".into(),
        boundary,
        wall_plate_ids: vec![WallPlateId(0)],
        posts: Vec::new(),
    });
    model.wall_weight_generation =
        Some(sepika_core::model::WallWeightGenerationMode::GravityCasesOnly);
    model
}

#[test]
fn 線形時刻歴は鋼板耐震壁を含むモデルで実行できる() {
    let mut app = App::default();
    app.load_model(steel_wall_model());
    for node in &mut app.core.model.nodes {
        node.mass = Some([1.0; 6]);
    }
    app.core.analysis_cfg.th_nonlinear = false;
    app.core.analysis_cfg.th_duration = 0.05;
    let wave = App::sample_wave(&app.core.analysis_cfg).unwrap();
    app.run_time_history(wave);
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    assert!(app
        .core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .time_history
        .is_some());
}

#[test]
fn 非線形時刻歴と増分解析では鋼板耐震壁を理由に停止する() {
    for background in [false, true] {
        for time_history in [false, true] {
            let mut app = App::default();
            app.load_model(steel_wall_model());
            app.generate_stories_action();
            for node in &mut app.core.model.nodes {
                node.mass = Some([1.0; 6]);
            }
            if time_history {
                app.core.analysis_cfg.th_nonlinear = true;
                app.core.analysis_cfg.th_duration = 0.05;
                let wave = App::sample_wave(&app.core.analysis_cfg).unwrap();
                if background {
                    app.start_time_history_job(wave);
                } else {
                    app.run_time_history(wave);
                }
            } else if background {
                app.start_pushover_job();
            } else {
                app.run_pushover();
            }
            if background {
                super::tests::wait_for_job(&mut app);
            }
            let error = app.core.scoped.last_error.as_deref().unwrap();
            assert!(
                error.contains("鋼板耐震壁 ID") && error.contains("非線形解析の対象外"),
                "{error}"
            );
            if let Some(results) = &app.core.scoped.results {
                assert!(results.time_history.is_none());
                assert!(results.pushover_x.is_none());
            }
        }
    }
}
