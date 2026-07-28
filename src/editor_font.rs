//! The font selectable for the Editor and Preview (`Settings`), applied
//! identically to both — one appearance choice, not a per-view knob, the same
//! reasoning as color themes. Deliberately a small, bundled set rather than a
//! live system-font picker: no filesystem/OS font-search dependency, and the
//! exact same choice on every platform. `LibertinusSerif`/`DejaVuSansMono` reuse
//! the exact same font files already embedded in tachylite for print-PDF export
//! (see `export::style`) — see `assets/fonts/NOTICE` for their licenses.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EditorFont {
    Proportional,
    Monospace,
    LibertinusSerif,
    DejaVuSansMono,
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
    pub const ALL: [EditorFont; 4] = [
        EditorFont::Proportional,
        EditorFont::Monospace,
        EditorFont::LibertinusSerif,
        EditorFont::DejaVuSansMono,
    ];

    pub fn label(self) -> &'static str {
        match self {
            EditorFont::Proportional => "Proportional",
            EditorFont::Monospace => "Monospace",
            EditorFont::LibertinusSerif => "Libertinus Serif",
            EditorFont::DejaVuSansMono => "DejaVu Sans Mono",
        }
    }

    /// The `egui::FontFamily` this resolves to, once `install` has registered
    /// the two bundled custom ones — egui's own default `Proportional`/
    /// `Monospace` families need no registration of their own.
    pub fn family(self) -> egui::FontFamily {
        match self {
            EditorFont::Proportional => egui::FontFamily::Proportional,
            EditorFont::Monospace => egui::FontFamily::Monospace,
            EditorFont::LibertinusSerif => egui::FontFamily::Name("LibertinusSerif".into()),
            EditorFont::DejaVuSansMono => egui::FontFamily::Name("DejaVuSansMono".into()),
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

/// Registers the two bundled custom font families with `ctx`'s font system —
/// call once at startup (`TachyliteApp::new`), before anything renders.
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
    ctx.set_fonts(fonts);
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
    fn install_registers_both_custom_families_and_does_not_panic() {
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
}
