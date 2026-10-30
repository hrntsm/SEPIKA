use sepika_app::app::App;

fn fixture_options(
    arguments: impl IntoIterator<Item = std::ffi::OsString>,
) -> Result<(std::path::PathBuf, [f32; 2]), String> {
    let mut arguments = arguments.into_iter();
    let project =
        std::path::PathBuf::from(arguments.next().ok_or("専用OVIKAの絶対パスが必要です")?);
    if !project.is_absolute() {
        return Err("専用OVIKAは絶対パスで指定してください".into());
    }
    let mut size = [1280.0, 800.0];
    if let Some(option) = arguments.next() {
        if option != "--size" {
            return Err("初期寸法は --size 幅 高さ で指定してください".into());
        }
        for (value, minimum) in size.iter_mut().zip([800.0, 600.0]) {
            *value = arguments
                .next()
                .and_then(|value| value.into_string().ok())
                .and_then(|value| value.parse::<f32>().ok())
                .filter(|value| value.is_finite() && *value >= minimum)
                .ok_or("初期寸法は有限値で幅800以上・高さ600以上が必要です")?;
        }
    }
    if arguments.next().is_some() {
        return Err("余分な引数があります".into());
    }
    Ok((project, size))
}

fn main() -> eframe::Result<()> {
    let (project, size) = fixture_options(std::env::args_os().skip(1)).unwrap_or_else(|reason| {
        eprintln!("{reason}");
        std::process::exit(2);
    });
    eframe::run_native(
        "SEPIKA #985 実モデル検討",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size(size)
                .with_min_inner_size([800.0, 600.0]),
            persist_window: false,
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
            app.restore_workspace_preferences();
            Ok(Box::new(app))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::fixture_options;

    fn options(arguments: &[&str]) -> Result<[f32; 2], String> {
        fixture_options(arguments.iter().map(std::ffi::OsString::from)).map(|(_, size)| size)
    }

    #[test]
    fn initial_window_dimensions_keep_default_and_accept_minimum() {
        let project = std::env::temp_dir().join("issue985.ovika");
        let project = project.to_str().unwrap();
        assert_eq!(options(&[project]).unwrap(), [1280.0, 800.0]);
        assert_eq!(
            options(&[project, "--size", "800", "600"]).unwrap(),
            [800.0, 600.0]
        );
    }

    #[test]
    fn invalid_window_arguments_are_rejected_before_gui_start() {
        let project = std::env::temp_dir().join("issue985.ovika");
        let project = project.to_str().unwrap();
        assert!(options(&[]).is_err());
        assert!(options(&["relative.ovika"]).is_err());
        for dimensions in [
            ["NaN", "600"],
            ["inf", "600"],
            ["0", "600"],
            ["-1", "600"],
            ["799", "600"],
            ["800", "599"],
            ["800", "invalid"],
        ] {
            assert!(options(&[project, "--size", dimensions[0], dimensions[1]]).is_err());
        }
        assert!(options(&[project, "--size", "800"]).is_err());
        assert!(options(&[project, "--unknown"]).is_err());
        assert!(options(&[project, "--size", "800", "600", "extra"]).is_err());
    }
}
