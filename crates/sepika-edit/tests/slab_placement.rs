use sepika_core::{frame_gen::*, ids::*};
use sepika_edit::*;

#[test]
fn 床板偏心の未入力零符号付き入力をundo_redoし階再採番も戻す() {
    let mut model = frame_model(&FrameSpec::default()).unwrap();
    let mut undo = UndoStack::default();
    let id = model
        .slabs
        .iter()
        .find(|s| s.reference_story == Some(StoryId(1)))
        .unwrap()
        .id;
    for value in [Some(100.), Some(-100.), None, Some(0.)] {
        let previous = model.slab(id).unwrap().vertical_eccentricity_mm;
        assert!(undo.run(
            &mut model,
            Box::new(SetSlabPlacement {
                id,
                reference_story: Some(StoryId(1)),
                vertical_eccentricity_mm: value
            })
        ));
        assert!(undo.affects_calculation());
        assert_eq!(model.slab(id).unwrap().vertical_eccentricity_mm, value);
        undo.undo(&mut model);
        assert_eq!(model.slab(id).unwrap().vertical_eccentricity_mm, previous);
        undo.redo(&mut model);
        assert_eq!(model.slab(id).unwrap().vertical_eccentricity_mm, value);
    }
    assert!(undo.run(
        &mut model,
        Box::new(AddStory {
            name: "中間階".into(),
            elevation: Some(2000.),
            sl_offset_mm: Some(0.)
        })
    ));
    assert_eq!(model.slab(id).unwrap().reference_story, Some(StoryId(2)));
    assert_eq!(model.resolved_slab_placement(id).unwrap().top_z_mm, 4000.);
    undo.undo(&mut model);
    assert_eq!(model.slab(id).unwrap().reference_story, Some(StoryId(1)));
    assert!(undo.run(&mut model, Box::new(DeleteStory { story: StoryId(1) })));
    assert_eq!(model.slab(id).unwrap().reference_story, None);
    assert!(model
        .resolved_slab_placement(id)
        .unwrap_err()
        .contains("基準階"));
    undo.undo(&mut model);
    assert_eq!(model.resolved_slab_placement(id).unwrap().top_z_mm, 4000.);
}

#[test]
fn 非有限偏心と存在しない階への編集は原入力を保つ() {
    let mut model = frame_model(&FrameSpec::default()).unwrap();
    let mut undo = UndoStack::default();
    let before = model.slabs[0].clone();
    for (reference_story, value) in [
        (Some(StoryId(999)), Some(0.)),
        (Some(StoryId(0)), Some(f64::NAN)),
    ] {
        assert!(!undo.run(
            &mut model,
            Box::new(SetSlabPlacement {
                id: SlabId(0),
                reference_story,
                vertical_eccentricity_mm: value
            })
        ));
        assert_eq!(model.slabs[0], before);
    }
}

#[test]
fn 参照切れと非有限偏心の修正はundoで元の不正入力も正確に復元する() {
    let mut model = frame_model(&FrameSpec::default()).unwrap();
    model.slabs[0].reference_story = Some(StoryId(999));
    model.slabs[0].vertical_eccentricity_mm = Some(f64::NAN);
    let mut undo = UndoStack::new();
    assert!(undo.run(
        &mut model,
        Box::new(SetSlabPlacement {
            id: SlabId(0),
            reference_story: Some(StoryId(0)),
            vertical_eccentricity_mm: Some(0.)
        })
    ));
    undo.undo(&mut model);
    assert_eq!(model.slabs[0].reference_story, Some(StoryId(999)));
    assert!(model.slabs[0].vertical_eccentricity_mm.unwrap().is_nan());
    undo.redo(&mut model);
    assert_eq!(
        model.resolved_slab_placement(SlabId(0)).unwrap().top_z_mm,
        0.
    );
}
