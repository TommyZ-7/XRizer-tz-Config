mod app;
mod core;
mod ui;

use std::ffi::OsString;
use std::path::PathBuf;

const FONT_ENV_VARS: [&str; 2] = ["TZ_CONTROLLER_FONT", "XRIZER_TZ_CONFIG_FONT"];

/// Locate the bundled font, preferring the new variable and falling back to
/// the variable exported by older AppRun scripts.
pub(crate) fn bundled_font_path() -> Option<PathBuf> {
    resolve_bundled_font(|name| std::env::var_os(name))
}

fn resolve_bundled_font(mut lookup: impl FnMut(&str) -> Option<OsString>) -> Option<PathBuf> {
    FONT_ENV_VARS
        .iter()
        .filter_map(|name| lookup(name))
        .map(PathBuf::from)
        .find(|path| path.is_file())
}

fn main() -> eframe::Result<()> {
    let bundled_font = bundled_font_path();
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 760.0])
            .with_min_inner_size([900.0, 600.0]),
        system_font_fallback: bundled_font.is_none(),
        ..Default::default()
    };
    eframe::run_native(
        "tz-controller",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup<'a>(
        entries: &'a [(&'a str, &'a std::path::Path)],
    ) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |name| {
            entries
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, path)| path.as_os_str().to_owned())
        }
    }

    #[test]
    fn bundled_font_prefers_tz_controller_variable() {
        let dir = tempfile::tempdir().unwrap();
        let new_font = dir.path().join("new.otf");
        let old_font = dir.path().join("old.otf");
        std::fs::write(&new_font, b"new").unwrap();
        std::fs::write(&old_font, b"old").unwrap();

        let resolved = resolve_bundled_font(lookup(&[
            ("TZ_CONTROLLER_FONT", &new_font),
            ("XRIZER_TZ_CONFIG_FONT", &old_font),
        ]));

        assert_eq!(resolved, Some(new_font));
    }

    #[test]
    fn bundled_font_falls_back_to_legacy_variable() {
        let dir = tempfile::tempdir().unwrap();
        let old_font = dir.path().join("old.otf");
        std::fs::write(&old_font, b"old").unwrap();

        let resolved = resolve_bundled_font(lookup(&[("XRIZER_TZ_CONFIG_FONT", &old_font)]));

        assert_eq!(resolved, Some(old_font));
    }

    #[test]
    fn bundled_font_skips_missing_files_and_uses_the_next_variable() {
        let dir = tempfile::tempdir().unwrap();
        let old_font = dir.path().join("old.otf");
        std::fs::write(&old_font, b"old").unwrap();
        let missing = dir.path().join("missing.otf");

        let resolved = resolve_bundled_font(lookup(&[
            ("TZ_CONTROLLER_FONT", &missing),
            ("XRIZER_TZ_CONFIG_FONT", &old_font),
        ]));

        assert_eq!(resolved, Some(old_font));
    }

    #[test]
    fn bundled_font_is_absent_when_no_variable_points_to_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.otf");

        let resolved = resolve_bundled_font(lookup(&[("TZ_CONTROLLER_FONT", &missing)]));

        assert_eq!(resolved, None);
    }
}
