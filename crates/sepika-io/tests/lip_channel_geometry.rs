use sepika_core::ids::SectionId;
use sepika_core::model::{FrameSectionUse, Model};
use sepika_core::section_shape::SectionShape;
use sepika_io::ovika::{load_ovika, save_ovika, OvikaExtras};

#[test]
fn lip_channel_geometry_ovika_roundtrip_and_public_save_rejection() {
    let mut model = Model::default();
    let mut section = SectionShape::SteelLipChannel {
        height: 150.0,
        width: 75.0,
        lip: 20.0,
        thick: 2.3,
    }
    .input_section(SectionId(0), "LC544".into())
    .unwrap();
    section.frame_use = Some(FrameSectionUse::Girder);
    model.sections.push(section);
    let path = std::env::temp_dir().join(format!(
        "sepika-lip-channel-geometry-{}.ovika",
        std::process::id(),
    ));
    save_ovika(&path, &model, OvikaExtras::default()).unwrap();
    let loaded = load_ovika(&path).unwrap().model;
    assert_eq!(loaded.sections[0].shape, model.sections[0].shape);
    assert!((loaded.sections[0].area - 760.84).abs() < 1e-9);
    assert!(model.eq_ignoring_dofmap(&loaded));
    let original = std::fs::read(&path).unwrap();
    for lip in [75.0, 75.001, 80.0, 100.0] {
        let shape = SectionShape::SteelLipChannel {
            height: 150.0,
            width: 75.0,
            lip,
            thick: 2.3,
        };
        let reason = shape
            .input_section(SectionId(0), "LC544".into())
            .unwrap_err();
        assert!(reason.contains("LC544") && reason.contains("H=150"));
        model.sections[0].shape = Some(shape);
        let reason = save_ovika(&path, &model, OvikaExtras::default())
            .unwrap_err()
            .to_string();
        assert!(reason.contains("LC544") && reason.contains("H=150"));
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert!(loaded.eq_ignoring_dofmap(&load_ovika(&path).unwrap().model));
    }
    std::fs::remove_file(path).unwrap();
}
