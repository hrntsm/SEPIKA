#[test]
fn ovika_roundtrip_preserves_the_steel_girder_composite_connection() {
    use sepika_core::ids::ElemId;
    use sepika_core::model::SlabCompositeConnection;

    let mut model = sepika_app::sample::portal_frame();
    let girder = ElemId(2);
    assert!(model.is_steel_h_girder(model.element(girder).unwrap()));
    assert_eq!(
        model.set_slab_composite_connection(girder, SlabCompositeConnection::NonComposite),
        SlabCompositeConnection::Unconfirmed
    );

    let path =
        std::env::temp_dir().join(format!("sepika-composite-{}.ovika", uuid::Uuid::new_v4()));
    sepika_io::ovika::save_ovika(&path, &model, Default::default()).unwrap();
    let restored = sepika_io::ovika::load_ovika(&path).unwrap().model;
    std::fs::remove_file(path).unwrap();

    assert!(restored.is_steel_h_girder(restored.element(girder).unwrap()));
    assert_eq!(
        restored.slab_composite_connection(girder),
        SlabCompositeConnection::NonComposite
    );
}
