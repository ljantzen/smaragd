//! Captures build-time metadata (git commit, working-tree cleanliness, build date)
//! as compile-time env vars, so `ui::about_panel` can show exactly which build is
//! running — useful for a dev tool with no auto-update/release channel of its own.
//! Shells out to `git` read-only (a metadata query, not a VCS operation) — safe
//! regardless of whether the repo is worked in day-to-day via git or jj, since jj
//! colocates with a real git store underneath.

use std::process::Command;

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(cmd).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|s| s.trim().to_string())
}

/// Window icon size in pixels (square); must match `ICON_SIZE` in `src/main.rs`.
const ICON_SIZE: u32 = 256;

/// Rasterizes `assets/smaragd-icon.svg` into raw RGBA8 bytes for the window icon,
/// so the icon has a single source of truth (the SVG) rather than a checked-in PNG.
fn generate_icon() {
    let svg_path = "assets/smaragd-icon.svg";
    println!("cargo:rerun-if-changed={svg_path}");

    let svg_data = std::fs::read(svg_path).expect("failed to read app icon SVG");
    let tree = usvg::Tree::from_data(&svg_data, &usvg::Options::default())
        .expect("failed to parse app icon SVG");

    let mut pixmap = tiny_skia::Pixmap::new(ICON_SIZE, ICON_SIZE).expect("invalid icon size");
    let scale = ICON_SIZE as f32 / tree.size().width();
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );

    // tiny-skia stores premultiplied alpha; egui::IconData wants it straight/unmultiplied.
    let mut rgba = pixmap.data().to_vec();
    for px in rgba.chunks_exact_mut(4) {
        let a = px[3] as u32;
        for c in &mut px[..3] {
            let numerator = *c as u32 * 255 + a / 2;
            *c = numerator.checked_div(a).unwrap_or(0).min(255) as u8;
        }
    }

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    std::fs::write(std::path::Path::new(&out_dir).join("icon_rgba.bin"), &rgba)
        .expect("failed to write rasterized icon");
}

fn main() {
    generate_icon();

    let git_hash =
        run("git", &["rev-parse", "--short=8", "HEAD"]).unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=SMARAGD_GIT_HASH={git_hash}");

    let dirty = run("git", &["status", "--porcelain"]).is_some_and(|s| !s.is_empty());
    println!(
        "cargo:rustc-env=SMARAGD_GIT_DIRTY={}",
        if dirty { "-dirty" } else { "" }
    );

    let build_date =
        run("date", &["-u", "+%Y-%m-%d %H:%M UTC"]).unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=SMARAGD_BUILD_DATE={build_date}");

    // Re-run this script (and thus refresh the above) whenever HEAD moves or the
    // index changes, rather than only when smaragd's own source changes.
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");
}
