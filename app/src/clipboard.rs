//! OS clipboard image access for pasting into the editor (`ui::editor_panel`).
//! Kept out of `smaragd_core::attachments` since it needs `arboard`/`image` —
//! native-integration dependencies `smaragd-core` doesn't carry, so it can be
//! swapped for a Tauri clipboard-plugin call later without touching `core`.

/// Fetch the OS clipboard's image, if any, re-encoded as PNG bytes.
/// `Ok(None)` means "clipboard has no image" — the normal case for every
/// ordinary text paste, not an error. `size_limit_bytes` (see
/// `Project::clipboard_image_size_limit_bytes`), when set, rejects an
/// over-sized image with `Err` instead of silently truncating it.
pub fn clipboard_image_png(size_limit_bytes: Option<u64>) -> Result<Option<Vec<u8>>, String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    let img = match clipboard.get_image() {
        Ok(img) => img,
        Err(arboard::Error::ContentNotAvailable) => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let rgba =
        image::RgbaImage::from_raw(img.width as u32, img.height as u32, img.bytes.into_owned())
            .ok_or("clipboard image data didn't match its reported dimensions")?;
    let mut png_bytes = Vec::new();
    image::DynamicImage::ImageRgba8(rgba)
        .write_to(
            &mut std::io::Cursor::new(&mut png_bytes),
            image::ImageFormat::Png,
        )
        .map_err(|e| e.to_string())?;
    if let Some(limit) = size_limit_bytes
        && png_bytes.len() as u64 > limit
    {
        let limit_mb = limit / (1024 * 1024);
        return Err(format!(
            "Clipboard image is larger than the configured {limit_mb} MB limit"
        ));
    }
    Ok(Some(png_bytes))
}
