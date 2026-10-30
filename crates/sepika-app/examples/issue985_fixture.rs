use sepika_app::app::App;

fn main() -> eframe::Result<()> {
    let project =
        std::path::PathBuf::from(std::env::args_os().nth(1).expect("専用OVIKAの絶対パス"));
    assert!(project.is_absolute());
    eframe::run_native(
        "SEPIKA #985 実モデル検討",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1280.0, 800.0])
                .with_min_inner_size([800.0, 600.0]),
            ..Default::default()
        },
        Box::new(move |cc| {
            sepika_app::app::install_japanese_fonts(&cc.egui_ctx);
            sepika_app::theme::apply_theme(&cc.egui_ctx);
            let mut app = App::default();
            app.open_project_from(project);
            assert!(
                app.core.scoped.last_error.is_none(),
                "{:?}",
                app.core.scoped.last_error
            );
            Ok(Box::new(app))
        }),
    )
}
