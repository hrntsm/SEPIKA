use sepika_app::app::App;
use sepika_core::ids::ElemId;

#[path = "support/holding_clear_span.rs"]
mod holding_clear_span;
use holding_clear_span::rc_rank_portal;
#[path = "../../sepika-solver/tests/support/rc_wall_side_column_model.rs"]
mod rc_fixture;

#[test]
fn rc_rank_and_ds_are_held_when_clear_span_is_unconfirmed() {
    for (target, circular, height_mm, beam_faces, expected) in [
        (
            ElemId(2),
            false,
            3000.0,
            Some((None, Some(0.0))),
            "i端のフェース距離が未算定",
        ),
        (
            ElemId(2),
            false,
            3000.0,
            Some((Some(0.0), None)),
            "j端のフェース距離が未算定",
        ),
        (
            ElemId(2),
            false,
            3000.0,
            Some((None, None)),
            "両端のフェース距離が未算定",
        ),
        (
            ElemId(2),
            false,
            3000.0,
            Some((Some(-1.0), Some(0.0))),
            "i端のフェース距離 -1 mm が有限な非負値ではありません",
        ),
        (
            ElemId(2),
            false,
            3000.0,
            Some((Some(0.0), Some(f64::INFINITY))),
            "j端のフェース距離 inf mm が有限な非負値ではありません",
        ),
        (ElemId(0), false, 600.0, None, "正の内法長さが成立しません"),
        (ElemId(0), true, 400.0, None, "正の内法長さが成立しません"),
    ] {
        let mut app = rc_rank_portal(
            rc_fixture::expanded_model(),
            circular,
            height_mm,
            beam_faces,
        );
        let section = if target == ElemId(0) { 0 } else { 1 };
        let member = format!("部材 ID {}・断面 S{section}（ID {section}）", target.0);
        let error = app.compute_holding_capacity().err().expect("保留");
        assert!(error.contains(&member), "{error}");
        assert!(error.contains(expected), "{error}");
        assert!(
            error.contains("自動部材種別・Ds・必要保有水平耐力・合否を保留します"),
            "{error}"
        );
        assert!(app.core.scoped.holding_capacity_source.is_none());

        #[cfg(feature = "gui")]
        {
            let output = egui::Context::default().run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(4000.0, 10000.0),
                    )),
                    ..Default::default()
                },
                |ui| sepika_app::design_view::design_table(ui, &mut app),
            );
            let visible = output.shapes.iter().any(
                |shape| matches!(&shape.shape, egui::Shape::Text(t) if t.galley.text() == error),
            );
            assert!(visible, "{error}");
        }

        app.core.design_rank_auto = false;
        let error = app.compute_holding_capacity().err().expect("保留");
        assert!(
            error.contains("全体・部分・局部崩壊形は採用保留"),
            "{error}"
        );
    }
}
