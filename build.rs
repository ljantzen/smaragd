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

/// Window icon size in pixels (square); must match `ICON_SIZE` in `src/main.rs`,
/// and the checked-in `assets/smaragd-icon.png`.
const ICON_SIZE: u32 = 256;

/// Decodes `assets/smaragd-icon.png` into raw RGBA8 bytes for the window icon,
/// so the icon has a single source of truth (the PNG) rather than a generated copy.
fn generate_icon() {
    let png_path = "assets/smaragd-icon.png";
    println!("cargo:rerun-if-changed={png_path}");

    let img = image::open(png_path)
        .expect("failed to read app icon PNG")
        .into_rgba8();
    assert_eq!(
        (img.width(), img.height()),
        (ICON_SIZE, ICON_SIZE),
        "{png_path} must be {ICON_SIZE}x{ICON_SIZE}"
    );

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    std::fs::write(
        std::path::Path::new(&out_dir).join("icon_rgba.bin"),
        img.into_raw(),
    )
    .expect("failed to write decoded icon");
}

/// Embeds `assets/smaragd-icon.png` as the .exe's icon (taskbar, Explorer, Alt-Tab)
/// via a compiled-in Windows resource. Built as a multi-size .ico on the fly rather
/// than checking one in, so this stays derived from the same single PNG as the
/// window icon above.
#[cfg(windows)]
fn embed_windows_icon() {
    use image::ExtendedColorType;
    use image::codecs::ico::{IcoEncoder, IcoFrame};
    use image::imageops::FilterType;

    let png_path = "assets/smaragd-icon.png";
    let img = image::open(png_path).expect("failed to read app icon PNG");

    let frames: Vec<IcoFrame> = [16u32, 32, 48, 256]
        .into_iter()
        .map(|size| {
            let resized = img
                .resize_exact(size, size, FilterType::Lanczos3)
                .into_rgba8();
            IcoFrame::as_png(resized.as_raw(), size, size, ExtendedColorType::Rgba8)
                .expect("failed to build ICO frame")
        })
        .collect();

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    let ico_path = std::path::Path::new(&out_dir).join("smaragd.ico");
    IcoEncoder::new(std::fs::File::create(&ico_path).expect("failed to create ICO file"))
        .encode_images(&frames)
        .expect("failed to encode ICO");

    winresource::WindowsResource::new()
        .set_icon(ico_path.to_str().expect("OUT_DIR path must be valid UTF-8"))
        .compile()
        .expect("failed to embed Windows icon resource");
}

#[cfg(not(windows))]
fn embed_windows_icon() {}

fn main() {
    generate_icon();
    embed_windows_icon();

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
