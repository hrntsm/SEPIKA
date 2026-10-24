use super::*;
use sepika_core::{frame_gen::*, ids::SlabId};

#[test]
fn 床板配置のgui編集保存再読込undo_redoと結果失効は同じ入力を使う() {
    let mut app = App::default();
    assert!(app.load_model(frame_model(&FrameSpec::default()).unwrap()));
    let id = SlabId(0);
    let story = app.core.model.slabs[0].reference_story;
    let supports = app.core.model.floor_assignment_regions.clone();
    for eccentricity in [Some(100.), Some(-100.), None, Some(0.)] {
        app.core.scoped.staleness.mark_fresh();
        app.core.scoped.staleness.preparation_stale = false;
        app.core.scoped.staleness.design_stale = false;
        let previous = app.core.model.slabs[0].vertical_eccentricity_mm;
        let input = app.result_input(&result_validity::ResultInputKey::Modal);
        assert!(
            app.apply_model_edit(Box::new(sepika_edit::SetSlabPlacement {
                id,
                reference_story: story,
                vertical_eccentricity_mm: eccentricity
            }))
        );
        assert!(
            app.core.scoped.staleness.results_stale
                && app.core.scoped.staleness.preparation_stale
                && app.core.scoped.staleness.design_stale
        );
        assert_ne!(
            app.result_input(&result_validity::ResultInputKey::Modal),
            input
        );
        app.undo_action();
        assert_eq!(app.core.model.slabs[0].vertical_eccentricity_mm, previous);
        app.redo_action();
        let path = std::env::temp_dir().join(format!(
            "sepika-727-{:?}-{}.ovika",
            eccentricity,
            std::process::id()
        ));
        sepika_io::ovika::save_ovika(&path, &app.core.model, Default::default()).unwrap();
        let restored = sepika_io::ovika::load_ovika(&path).unwrap().model;
        assert_eq!(restored.slabs[0].vertical_eccentricity_mm, eccentricity);
        assert_eq!(restored.slabs[0].reference_story, story);
        assert_eq!(restored.floor_assignment_regions, supports);
        match eccentricity {
            Some(value) => assert_eq!(
                restored.resolved_slab_placement(id).unwrap().top_z_mm,
                value
            ),
            None => assert!(restored
                .resolved_slab_placement(id)
                .unwrap_err()
                .contains("鉛直偏心")),
        }
        std::fs::remove_file(path).unwrap();
    }
}
