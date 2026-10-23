//! Embedded font bytes shared by export rendering (`export::latex`,
//! `export::pdf`) and the app's egui font registration (`editor_font::install`).
//! Split out from `editor_font` so `core` doesn't need to depend on `egui` just
//! to reuse these bytes.

/// `export::latex` reuses these bytes as the `Regular` weight of its own
/// bundled "Libertinus Serif" family (see `export::latex::bundled_font_family`).
pub const LIBERTINUS_SERIF: &[u8] =
    include_bytes!("../../assets/fonts/LibertinusSerif-Regular.otf");
/// `export::latex` reuses these bytes as the `Regular` weight of its own
/// bundled "DejaVu Sans Mono" family.
pub const DEJAVU_SANS_MONO: &[u8] = include_bytes!("../../assets/fonts/DejaVuSansMono.ttf");
/// `export::pdf` reuses these bytes to register this font with the Typst
/// compiler too, since it (unlike the other two) isn't already part of
/// `typst-kit`'s embedded font set.
pub const ATKINSON_HYPERLEGIBLE: &[u8] =
    include_bytes!("../../assets/fonts/AtkinsonHyperlegible-Regular.ttf");
