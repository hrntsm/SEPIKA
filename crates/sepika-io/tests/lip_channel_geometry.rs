use sepika_core::ids::SectionId;
use sepika_core::model::{FrameSectionUse, Model};
use sepika_core::section_shape::SectionShape;
use sepika_io::stbridge::{
    export_stbridge, export_stbridge_with_report, import_stbridge, StbError,
};

#[test]
fn lip_channel_geometry_stbridge_roundtrip_and_public_import_export_rejection() {
    let mut model = Model::default();
    let mut section = SectionShape::SteelLipChannel {
        height: 150.0,
        width: 75.0,
        lip: 20.0,
        thick: 2.3,
    }
    .to_section(SectionId(0), "LC544".into());
    section.frame_use = Some(FrameSectionUse::Girder);
    model.sections.push(section);
    let xml = export_stbridge(&model).unwrap();
    assert!(xml.contains("LipC"), "{xml}");
    let loaded = import_stbridge(&xml).unwrap();
    assert_eq!(loaded.sections[0].shape, model.sections[0].shape);
    assert!((loaded.sections[0].area - 760.84).abs() < 1e-9);
    for lip in [75.0, 75.001, 80.0, 100.0] {
        let invalid_xml = xml.replace("C=\"20\"", &format!("C=\"{lip}\""));
        assert_ne!(invalid_xml, xml);
        let error = import_stbridge(&invalid_xml).unwrap_err();
        assert!(matches!(error, StbError::Unmappable(_)), "{error}");
        assert!(error.to_string().contains("LC544") && error.to_string().contains("H=150"));
        model.sections[0].shape = Some(SectionShape::SteelLipChannel {
            height: 150.0,
            width: 75.0,
            lip,
            thick: 2.3,
        });
        for result in [
            export_stbridge(&model),
            export_stbridge_with_report(&model).map(|(xml, _)| xml),
        ] {
            assert!(
                matches!(result, Err(StbError::Unmappable(reason)) if reason.contains("LC544") && reason.contains("H=150"))
            );
        }
    }
}
