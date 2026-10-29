use sepika_app::app::App;
use sepika_core::{
    ids::{ElemId, SectionId},
    model::{FrameSectionUse, PanelZoneMode},
    section_shape::SectionShape,
};

fn main() -> eframe::Result<()> {
    eframe::run_native(
        "SEPIKA #982 接続確認",
        eframe::NativeOptions::default(),
        Box::new(|cc| {
            sepika_app::app::install_japanese_fonts(&cc.egui_ctx);
            sepika_app::theme::apply_theme(&cc.egui_ctx);
            let mut app = App::default();
            let mut model = sepika_app::sample::portal_frame();
            model.panel_zone = PanelZoneMode::None;
            let mut section = SectionShape::SteelH {
                root_r: Some(13.0),
                height: 500.0,
                width: 250.0,
                web_thick: 10.0,
                flange_thick: 16.0,
            }
            .to_section(SectionId(2), "比較用大梁".into());
            section.frame_use = Some(FrameSectionUse::Girder);
            section.material = model.sections[1].material;
            model.sections.push(section);
            assert!(app.load_model(model));
            app.core.analysis_cfg.threads = 1;
            app.create_study("外部AI接続の正常検証".into(), vec![ElemId(2)])
                .unwrap();
            Ok(Box::new(app))
        }),
    )
}
