use sepika_app::sample::portal_frame;
use sepika_core::ids::ElemId;
use sepika_core::model::{AnalysisKind, DamperAttr, DamperProps, ElementData, ElementKind, Model};
use sepika_edit::{DeleteMember, UndoStack};
use sepika_element::behavior::{Ctx, LocalVec};
use sepika_element::factory::{
    build_nonlinear_behavior, ensure_nonlinear_input_for_kind, StrengthBasis,
};
use sepika_io::ovika::{load_ovika, save_ovika};

fn assert_ovika_roundtrip(
    model: &Model,
    elements: &[ElementData],
    attrs: &[DamperAttr],
    expected_response: &[(f64, f64)],
) {
    let path = std::env::temp_dir().join(format!("sepika-damper-{}.ovika", uuid::Uuid::new_v4()));
    save_ovika(&path, model, Default::default()).unwrap();
    let reread = load_ovika(&path).unwrap().model;
    std::fs::remove_file(path).unwrap();

    for model in [model, &reread] {
        assert_eq!(model.elements.as_slice(), elements);
        assert_eq!(model.damper_attrs.as_slice(), attrs);
        assert_eq!(model.elements.len(), expected_response.len());
        let kind = AnalysisKind::TimeHistory;
        ensure_nonlinear_input_for_kind(model, kind).unwrap();
        let ctx = Ctx { model };
        let mut increment = LocalVec {
            data: smallvec::SmallVec::from_elem(0.0, 12),
        };
        increment.data[8] = 1.0;
        // Kd=100 kN/mm、刻み0.01 s、初期変形0から1 mm伸ばしたときの軸力。
        for (elem, &(c0, force_n)) in model.elements.iter().zip(expected_response) {
            assert_eq!(model.damper_props(elem.id).unwrap().c0, c0);
            let mut behavior =
                build_nonlinear_behavior(elem, model, StrengthBasis::Nominal, kind, None);
            behavior.set_time_step(0.01);
            behavior.update_state(&increment, true, &ctx).unwrap();
            let computed_n = behavior.state_member_forces(&ctx).unwrap().at[0].1[0];
            assert!(
                (computed_n - force_n).abs() < 1e-9,
                "{computed_n} != {force_n}"
            );
        }
    }
}

#[test]
fn deleting_a_damper_preserves_the_retained_dampers_response_across_undo_redo_and_ovika() {
    let sample = portal_frame();
    let mut model = Model {
        nodes: sample.nodes,
        elements: sample.elements,
        ..Default::default()
    };
    model.elements.truncate(2);
    for elem in &mut model.elements {
        elem.kind = ElementKind::Damper;
        elem.section = None;
    }
    let attrs = [(0, 1000.0), (1, 3000.0), (0, 2000.0)].map(|(id, c0)| DamperAttr {
        elem: ElemId(id),
        props: DamperProps {
            c0,
            alpha: 1.0,
            ..Default::default()
        },
    });
    model.damper_attrs = attrs.to_vec();
    let elements = model.elements.clone();
    let remaining_element = ElementData {
        id: ElemId(0),
        ..elements[1].clone()
    };
    let remaining_attr = DamperAttr {
        elem: ElemId(0),
        ..attrs[1]
    };
    let original_response = [(1000.0, 50_000.0), (3000.0, 75_000.0)];
    let remaining_response = [(3000.0, 75_000.0)];
    assert_ovika_roundtrip(&model, &elements, &attrs, &original_response);

    let mut edits = UndoStack::new();
    assert!(edits.run(&mut model, Box::new(DeleteMember { id: ElemId(0) })));
    assert_ovika_roundtrip(
        &model,
        std::slice::from_ref(&remaining_element),
        &[remaining_attr],
        &remaining_response,
    );
    edits.undo(&mut model);
    assert_ovika_roundtrip(&model, &elements, &attrs, &original_response);
    edits.redo(&mut model);
    assert_ovika_roundtrip(
        &model,
        &[remaining_element],
        &[remaining_attr],
        &remaining_response,
    );
}
