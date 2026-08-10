//! The font selectable for the Editor (`Settings::editor_font`). Deliberately a
//! small, bundled set rather than a live system-font picker: no filesystem/OS
//! font-search dependency, and the exact same choice on every platform.
//! `LibertinusSerif`/`DejaVuSansMono` reuse the exact same font files already
//! embedded in smaragd for print-PDF export (see `export::style`);
//! `AtkinsonHyperlegible` is bundled independently (not part of `typst-kit`'s
//! embedded set) and registered with the Typst compiler directly in
//! `export::pdf` so it renders identically there too — see
//! `assets/fonts/NOTICE` for all three fonts' licenses. The Preview tab also
//! draws on these registered families (via
//! `ui::markdown_preview::resolve_family`) when the selected `TypesetStyle`
//! names one of them, but Preview's font choice itself comes from that style,
//! not from `Settings::editor_font`.

use std::path::PathBuf;
use std::sync::Arc;

use egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EditorFont {
    Proportional,
    Monospace,
    LibertinusSerif,
    DejaVuSansMono,
    AtkinsonHyperlegible,
}

impl Default for EditorFont {
    /// Matches the Preview's own previous hardcoded font (egui's default
    /// proportional face) — of the two views this replaces a single hardcoded
    /// choice for, the Editor's own previous default (`Monospace`, via
    /// `.code_editor()`) is the one that changes on upgrade, not the Preview's.
    fn default() -> Self {
        Self::Proportional
    }
}

impl EditorFont {
    pub const ALL: [EditorFont; 5] = [
        EditorFont::Proportional,
        EditorFont::Monospace,
        EditorFont::LibertinusSerif,
        EditorFont::DejaVuSansMono,
        EditorFont::AtkinsonHyperlegible,
    ];

    pub fn label(self) -> &'static str {
        match self {
            EditorFont::Proportional => "Proportional",
            EditorFont::Monospace => "Monospace",
            EditorFont::LibertinusSerif => "Libertinus Serif",
            EditorFont::DejaVuSansMono => "DejaVu Sans Mono",
            EditorFont::AtkinsonHyperlegible => "Atkinson Hyperlegible",
        }
    }

    /// The `egui::FontFamily` this resolves to, once `install` has registered
    /// the three bundled custom ones — egui's own default `Proportional`/
    /// `Monospace` families need no registration of their own.
    pub fn family(self) -> egui::FontFamily {
        match self {
            EditorFont::Proportional => egui::FontFamily::Proportional,
            EditorFont::Monospace => egui::FontFamily::Monospace,
            EditorFont::LibertinusSerif => egui::FontFamily::Name("LibertinusSerif".into()),
            EditorFont::DejaVuSansMono => egui::FontFamily::Name("DejaVuSansMono".into()),
            EditorFont::AtkinsonHyperlegible => {
                egui::FontFamily::Name("AtkinsonHyperlegible".into())
            }
        }
    }

    pub fn font_id(self, size: f32) -> egui::FontId {
        egui::FontId::new(size, self.family())
    }
}

/// `Settings::editor_font_size`'s effective default, used whenever that field is
/// `0.0` (unconfigured — see its own doc comment). Splits the difference between
/// the Editor's previous size (egui's default `Monospace` `TextStyle`, ~13pt) and
/// the Preview's previous hardcoded 15pt, since they're now one shared setting
/// rather than two independent hardcoded values.
pub const DEFAULT_FONT_SIZE: f32 = 14.0;

/// Resolves `Settings::editor_font_size` to an actual point size, falling back to
/// `DEFAULT_FONT_SIZE` for `0.0` (unconfigured) or any other non-positive value.
pub fn resolve_size(configured: f32) -> f32 {
    if configured > 0.0 {
        configured
    } else {
        DEFAULT_FONT_SIZE
    }
}

const LIBERTINUS_SERIF: &[u8] = include_bytes!("../assets/fonts/LibertinusSerif-Regular.otf");
const DEJAVU_SANS_MONO: &[u8] = include_bytes!("../assets/fonts/DejaVuSansMono.ttf");
/// `pub(crate)` (not just this module's own `install`) — `export::pdf` reuses
/// the exact same bytes to register this font with the Typst compiler too,
/// since it (unlike the other two) isn't already part of `typst-kit`'s
/// embedded font set.
pub(crate) const ATKINSON_HYPERLEGIBLE: &[u8] =
    include_bytes!("../assets/fonts/AtkinsonHyperlegible-Regular.ttf");

/// Registers the three bundled custom font families with `ctx`'s font system —
/// call once at startup (`SmaragdApp::new`), before anything renders.
pub fn install(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "LibertinusSerif".to_owned(),
        Arc::new(egui::FontData::from_static(LIBERTINUS_SERIF)),
    );
    fonts.font_data.insert(
        "DejaVuSansMono".to_owned(),
        Arc::new(egui::FontData::from_static(DEJAVU_SANS_MONO)),
    );
    fonts.font_data.insert(
        "AtkinsonHyperlegible".to_owned(),
        Arc::new(egui::FontData::from_static(ATKINSON_HYPERLEGIBLE)),
    );
    fonts
        .families
        .entry(egui::FontFamily::Name("LibertinusSerif".into()))
        .or_default()
        .insert(0, "LibertinusSerif".to_owned());
    fonts
        .families
        .entry(egui::FontFamily::Name("DejaVuSansMono".into()))
        .or_default()
        .insert(0, "DejaVuSansMono".to_owned());
    fonts
        .families
        .entry(egui::FontFamily::Name("AtkinsonHyperlegible".into()))
        .or_default()
        .insert(0, "AtkinsonHyperlegible".to_owned());
    ctx.set_fonts(fonts);
}

/// Load and register every custom font file a loaded `TypesetStyle` declares via
/// `font_file` (see `export::style::custom_font_files`) with `ctx`'s font
/// system, under a family named after the font's own declared name — so
/// `ui::markdown_preview::resolve_family` can find it by that same name, same as
/// it already does for the three bundled fonts. Uses `Context::add_font`
/// (additive, and a no-op for a name already registered) instead of rebuilding
/// the whole `FontDefinitions` via `set_fonts` like `install` does — so this can
/// be called again on every "Reload Custom Styles" without disturbing `install`'s
/// bundled fonts or re-registering a font that's already loaded.
///
/// Each file is validated with `ttf_parser` *before* being handed to egui — a
/// malformed font isn't skipped gracefully by `epaint`, it `panic!`s the next
/// time anything tries to render with it (`Fonts::font` has no way to report a
/// "could not render" error once text layout is underway). Validating here
/// means a bad custom font degrades the same way a bad custom style/theme
/// `.toml` does — skipped, with a message — instead of crashing the app the
/// next time its style is previewed.
///
/// Returns `(registered, errors)`: `registered` is every name actually
/// registered — the only names `ui::markdown_preview::resolve_family` may build
/// an `egui::FontFamily::Name` from, never optimistically assuming a style's
/// `font_file` succeeded just because it was set; `errors` describes why any
/// others weren't.
pub fn install_custom_fonts(
    ctx: &egui::Context,
    fonts: &[(String, PathBuf)],
) -> (Vec<String>, Vec<String>) {
    let mut registered = Vec::new();
    let mut errors = Vec::new();
    for (name, path) in fonts {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(err) => {
                errors.push(format!("{name}: couldn't read {}: {err}", path.display()));
                continue;
            }
        };
        if let Err(err) = ttf_parser::Face::parse(&bytes, 0) {
            errors.push(format!(
                "{name}: not a valid font file ({}): {err}",
                path.display()
            ));
            continue;
        }
        ctx.add_font(FontInsert::new(
            name,
            egui::FontData::from_owned(bytes),
            vec![InsertFontFamily {
                family: egui::FontFamily::Name(name.clone().into()),
                priority: FontPriority::Highest,
            }],
        ));
        registered.push(name.clone());
    }
    (registered, errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_font_has_a_distinct_label() {
        let labels: Vec<&str> = EditorFont::ALL.iter().map(|f| f.label()).collect();
        let mut unique = labels.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(labels.len(), unique.len());
    }

    #[test]
    fn install_registers_all_custom_families_and_does_not_panic() {
        let ctx = egui::Context::default();
        install(&ctx);
        // A frame that actually lays out text in each custom family confirms
        // the font data was accepted and resolves to real glyphs, not just that
        // `set_fonts` didn't panic.
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 400.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| {
            for font in EditorFont::ALL {
                ui.label(egui::RichText::new("Sample").font(font.font_id(14.0)));
            }
        });
    }

    #[test]
    fn install_custom_fonts_registers_a_valid_font_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("MyFont.ttf");
        std::fs::write(&path, DEJAVU_SANS_MONO).unwrap();

        let ctx = egui::Context::default();
        let (registered, errors) =
            install_custom_fonts(&ctx, &[("My Custom Font".to_string(), path)]);

        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        assert_eq!(registered, vec!["My Custom Font".to_string()]);

        // Confirm the registered family actually resolves without panicking.
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 400.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| {
            ui.label(egui::RichText::new("Sample").font(egui::FontId::new(
                14.0,
                egui::FontFamily::Name("My Custom Font".into()),
            )));
        });
    }

    #[test]
    fn install_custom_fonts_skips_a_file_that_isnt_a_real_font() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("not_a_font.ttf");
        std::fs::write(&path, b"this is not font data").unwrap();

        let ctx = egui::Context::default();
        let (registered, errors) = install_custom_fonts(&ctx, &[("Bogus".to_string(), path)]);

        assert!(registered.is_empty());
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("Bogus"));
    }

    #[test]
    fn install_custom_fonts_skips_a_missing_file() {
        let ctx = egui::Context::default();
        let (registered, errors) = install_custom_fonts(
            &ctx,
            &[("Missing".to_string(), PathBuf::from("/does/not/exist.ttf"))],
        );

        assert!(registered.is_empty());
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("Missing"));
    }

    #[test]
    fn install_custom_fonts_is_idempotent_across_repeated_calls() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("MyFont.ttf");
        std::fs::write(&path, DEJAVU_SANS_MONO).unwrap();

        let ctx = egui::Context::default();
        let fonts = [("My Custom Font".to_string(), path)];
        let (first, _) = install_custom_fonts(&ctx, &fonts);
        let (second, _) = install_custom_fonts(&ctx, &fonts);
        assert_eq!(first, second);
    }
}
