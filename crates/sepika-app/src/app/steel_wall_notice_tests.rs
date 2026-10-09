use super::*;
use sepika_core::ids::{WallPlateId, WallRegionId};
use sepika_core::model::{ElementKind, WallPlate, WallPlateShape, WallRegion};

pub(super) fn steel_wall_model() -> sepika_core::model::Model {
    let mut model = crate::sample::portal_frame();
    let mut bottom = model.elements[2].clone();
    bottom.id = ElemId(3);
    bottom.nodes = [NodeId(0), NodeId(1)].into_iter().collect();
    model.elements.push(bottom);
    let mut section = model.sections[1].clone();
    section.id = SectionId(2);
    section.name = "鋼板壁".into();
    section.frame_use = None;
    section.shape = None;
    section.thickness = Some(6.0);
    model.sections.push(section);
    let boundary = vec![NodeId(0), NodeId(1), NodeId(3), NodeId(2)];
    model.add_enclosed_wall_plate_from_nodes(
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
    model.wall_regions.push(WallRegion {
        id: WallRegionId(0),
        name: "鋼板壁構面".into(),
        boundary,
        wall_plate_ids: vec![WallPlateId(0)],
        posts: Vec::new(),
    });
    model
}

pub(super) fn wall_notices(app: &App) -> Vec<&str> {
    app.core
        .log
        .entries
        .iter()
        .filter(|entry| entry.level == LogLevel::Notice && entry.message.contains("せん断座屈"))
        .map(|entry| entry.message.as_str())
        .collect()
}

#[test]
fn 時刻歴の同期非同期と線形非線形と対象有無で鋼板壁注意を切り替える() {
    let mut expected = None;
    for background in [false, true] {
        for nonlinear in [false, true] {
            for has_wall in [false, true] {
                let mut app = App::default();
                app.load_model(if has_wall {
                    steel_wall_model()
                } else {
                    crate::sample::portal_frame()
                });
                for node in &mut app.core.model.nodes {
                    node.mass = Some([1.0; 6]);
                }
                app.core.analysis_cfg.th_nonlinear = nonlinear;
                app.core.analysis_cfg.th_duration = 0.05;
                let wave = App::sample_wave(&app.core.analysis_cfg);
                if background {
                    app.start_time_history_job(wave);
                    assert!(
                        app.core.scoped.job.is_some(),
                        "{:?}",
                        app.core.scoped.last_error
                    );
                    assert_eq!(wall_notices(&app).len(), usize::from(nonlinear && has_wall));
                    super::tests::wait_for_job(&mut app);
                } else {
                    app.run_time_history(wave);
                }
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
                assert_eq!(wall_notices(&app).len(), usize::from(nonlinear && has_wall));
                if nonlinear && has_wall {
                    let notice = wall_notices(&app)[0].to_owned();
                    assert!(notice.contains("1 枚（要素 ID:"));
                    assert!(notice.contains("壁版 ID: 0"));
                    assert!(notice.contains("Qy=t·lw·F/√3"));
                    assert!(notice.contains("耐力を過大評価"));
                    if let Some(expected) = &expected {
                        assert_eq!(&notice, expected);
                    } else {
                        expected = Some(notice);
                    }
                }
            }
        }
    }
}

#[test]
fn 非線形時刻歴の再起動で再通知し準備注意を消さない() {
    for background in [false, true] {
        let mut app = App::default();
        app.load_model(steel_wall_model());
        app.generate_stories_action();
        for node in &mut app.core.model.nodes {
            node.mass = Some([1.0; 6]);
        }
        app.core.analysis_cfg.th_nonlinear = true;
        app.core.analysis_cfg.ai_mode = AiMode::SemiPrecise;
        app.core.analysis_cfg.th_duration = 0.05;
        for count in 1..=2 {
            let wave = App::sample_wave(&app.core.analysis_cfg);
            if background {
                app.start_time_history_job(wave);
                super::tests::wait_for_job(&mut app);
            } else {
                app.run_time_history(wave);
            }
            assert!(
                app.core.scoped.last_error.is_none(),
                "{:?}",
                app.core.scoped.last_error
            );
            assert_eq!(wall_notices(&app).len(), count);
            let notice = app.core.scoped.last_notice.as_ref().unwrap();
            assert!(notice.contains("せん断座屈"));
            if count == 1 {
                assert!(notice.contains("固有値"), "{notice}");
            }
        }
    }
}

#[test]
fn 鋼板壁注意と時刻歴計算エラーをともに保持する() {
    for background in [false, true] {
        let mut app = App::default();
        app.load_model(steel_wall_model());
        app.core.analysis_cfg.th_nonlinear = true;
        let mut wave = App::sample_wave(&app.core.analysis_cfg);
        wave.dt = 0.0;
        if background {
            app.start_time_history_job(wave);
            super::tests::wait_for_job(&mut app);
        } else {
            app.run_time_history(wave);
        }
        assert!(app.core.scoped.last_error.is_some());
        assert_eq!(wall_notices(&app).len(), 1);
        assert!(app
            .core
            .scoped
            .last_notice
            .as_ref()
            .unwrap()
            .contains("せん断座屈"));
    }
}

#[test]
fn 増分解析の同期非同期でも生成鋼板壁注意を維持する() {
    for background in [false, true] {
        let mut app = App::default();
        app.load_model(steel_wall_model());
        app.generate_stories_action();
        app.core.analysis_cfg.push_steps = 2;
        if background {
            app.start_pushover_job();
            super::tests::wait_for_job(&mut app);
        } else {
            app.run_pushover();
        }
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        assert!(app.core.scoped.results.as_ref().unwrap().pushover.is_some());
        assert_eq!(wall_notices(&app).len(), 1);
        assert!(app
            .core
            .scoped
            .last_notice
            .as_ref()
            .unwrap()
            .contains("せん断座屈"));
    }
}

#[test]
fn 共通通知は明示壁と生成壁を集約しrcとsrc内蔵鋼板と要素にならない版を除外する() {
    let input = steel_wall_model();
    let (mut expanded, index, _) = sepika_load::wall_expand::expand_wall_elements(&input);
    let mut wall = expanded.elements.last().unwrap().clone();
    let first_id = wall.id.0;
    wall.id = ElemId(first_id + 1);
    expanded.elements.push(wall);
    let notice = sepika_job::notices::steel_seismic_wall_notice(&expanded, &index).unwrap();
    assert!(notice.contains(&format!(
        "2 枚（要素 ID: {first_id}（壁版 ID: 0）, {}）",
        first_id + 1
    )));
    expanded.sections[2].shape = Some(sepika_core::section_shape::SectionShape::RcWall {
        thickness: 180.0,
        ps: 0.0025,
    });
    assert!(sepika_job::notices::steel_seismic_wall_notice(&expanded, &index).is_none());
    expanded.sections[2].steel_material = Some(MaterialId(0));
    assert!(sepika_job::notices::steel_seismic_wall_notice(&expanded, &index).is_none());
    let mut input = input;
    input.wall_plates[0].slit.beam_face[0] = true;
    let (expanded, index, _) = sepika_load::wall_expand::expand_wall_elements(&input);
    assert!(sepika_job::notices::steel_seismic_wall_notice(&expanded, &index).is_none());
    input.wall_plates[0].section = None;
    let (expanded, index, report) = sepika_load::wall_expand::expand_wall_elements(&input);
    assert_eq!(report.generated, 0);
    assert!(sepika_job::notices::steel_seismic_wall_notice(&expanded, &index).is_none());
    assert!(expanded
        .elements
        .iter()
        .all(|element| element.kind != ElementKind::Wall));
}
