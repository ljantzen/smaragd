//! Saving a pasted/dropped binary attachment (image, PDF, ...) to disk and
//! building the markdown snippet to insert at the cursor — the shared
//! terminal step for all three input paths `ui::editor_panel::show` wires up
//! (clipboard image, pasted file path/URI, drag-and-drop). See
//! `project::attachments` for the per-project "where" setting this consults.

use std::io;
use std::path::{Path, PathBuf};

use crate::markdown::has_image_extension;
use crate::project::store::ProjectStore;

/// True if `name`'s extension is one recognized as an image — same list the
/// markdown parser uses for `![[Target]]` embeds (`markdown::IMAGE_EXTENSIONS`),
/// so a pasted/dropped file is treated as an image exactly when the preview
/// would already treat a wikilink to it that way.
pub fn is_image_filename(name: &str) -> bool {
    has_image_extension(name)
}

/// CommonMark's bracket-destination form is used literally by
/// `ui::markdown_preview::resolve_image_uri` — a bare destination containing a
/// space or parenthesis (e.g. `unique_child_name`'s collision suffix,
/// `"Screenshot (2).png"`) isn't valid CommonMark syntax there, so it must be
/// wrapped in `<...>`. Percent-encoding is not an alternative: `resolve_image_uri`
/// treats the destination as a literal filesystem path, so a `%20` would be
/// looked up verbatim and fail to resolve. Only non-image links need this — see
/// `markdown_snippet`.
fn needs_angle_brackets(path: &str) -> bool {
    path.contains(' ') || path.contains('(') || path.contains(')')
}

/// Build the markdown snippet to insert for an attachment already saved at
/// `relative_path` (forward-slash-normalized, relative to the document doing
/// the inserting). `display_name` is the link text for a non-image file.
///
/// An image gets an Obsidian-style `![[Target]]` embed — `markdown::expand_placeholders`
/// resolves `src` from the raw text between the brackets exactly the same way it
/// resolves a standard `![](src)` image (see `ui::markdown_preview::resolve_image_uri`:
/// both end up as the same `ImageRef`), so this needs no CommonMark destination
/// escaping at all: a path with spaces or parens works unescaped inside `[[...]]`.
/// A non-image file keeps the standard link form — `![[Target]]` whose target isn't
/// a recognized image extension falls back to a plain *wikilink* (a search for a note
/// titled `Target`), not a working file link, which is the wrong behavior for a PDF
/// or other attachment.
pub fn markdown_snippet(relative_path: &str, display_name: &str, is_image: bool) -> String {
    if is_image {
        format!("![[{relative_path}]]")
    } else {
        let destination = if needs_angle_brackets(relative_path) {
            format!("<{relative_path}>")
        } else {
            relative_path.to_string()
        };
        format!("[{display_name}]({destination})")
    }
}

/// Relativize `target` (an absolute path) against `from` (an absolute
/// directory), as a forward-slash-joined string — markdown links are always
/// `/`-separated regardless of platform. Hand-rolled rather than a dependency,
/// consistent with `project::relative_key`'s own hand-rolled path handling.
pub fn relative_markdown_path(from: &Path, target: &Path) -> String {
    let from_components: Vec<_> = from.components().collect();
    let target_components: Vec<_> = target.components().collect();
    let common = from_components
        .iter()
        .zip(target_components.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let ups = from_components.len() - common;
    let mut parts: Vec<String> = Vec::with_capacity(ups + target_components.len() - common);
    parts.extend(std::iter::repeat_n("..".to_string(), ups));
    parts.extend(
        target_components[common..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().into_owned()),
    );
    if parts.is_empty() {
        target
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    } else {
        parts.join("/")
    }
}

/// Suggest a filename for clipboard image data, which carries none of its
/// own — `unique_child_name` dedupes it further on an actual collision, so
/// this only needs to be parseable and collision-unlikely, not guaranteed
/// unique by itself.
pub fn suggested_clipboard_image_name(now: chrono::DateTime<chrono::Local>) -> String {
    format!("pasted-image-{}.png", now.format("%Y%m%d-%H%M%S"))
}

/// If `text` is exactly one line that's either a `file://` URI or a bare
/// absolute path, and it resolves to an existing file, returns that path —
/// else `None` (ordinary text, to be inserted literally as today). The
/// *entire* trimmed text must be the path, not a substring of a longer paste.
pub fn pasted_text_as_file_path(text: &str) -> Option<PathBuf> {
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed.contains('\n') {
        return None;
    }
    let path = match trimmed.strip_prefix("file://") {
        Some(rest) => PathBuf::from(percent_decode(rest)),
        None => PathBuf::from(trimmed),
    };
    if path.is_absolute() && path.is_file() {
        Some(path)
    } else {
        None
    }
}

/// Decodes `%XX` escapes in a `file://` URI's path portion. Only handles the
/// one direction this module needs (URI -> filesystem path), not a general
/// percent-encoding implementation.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(byte) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The result of successfully saving an attachment: the markdown snippet to
/// splice into the buffer, and where the file actually landed (for the
/// caller's own diagnostics, not needed to build the snippet).
pub struct AttachmentSave {
    pub snippet: String,
    pub path: PathBuf,
}

/// Writes `bytes` into `dest_dir` (creating it if needed) under a
/// collision-safe variant of `suggested_name`, and builds the markdown
/// snippet linking to it from `document_dir`. `document_dir` and `dest_dir`
/// are the same path in `AttachmentDestination::SameAsDocument` mode; they
/// differ when a configured attachments folder is in use.
pub fn save_attachment(
    store: &dyn ProjectStore,
    dest_dir: &Path,
    document_dir: &Path,
    suggested_name: &str,
    bytes: &[u8],
) -> io::Result<AttachmentSave> {
    store.create_dir_all(dest_dir)?;
    let name = crate::project::unique_child_name(store, dest_dir, suggested_name);
    let path = dest_dir.join(&name);
    store.write(&path, bytes)?;
    let relative = relative_markdown_path(document_dir, &path);
    let snippet = markdown_snippet(&relative, &name, is_image_filename(&name));
    Ok(AttachmentSave { snippet, path })
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_snippet_builds_a_wikilink_image_embed() {
        assert_eq!(markdown_snippet("pic.png", "pic.png", true), "![[pic.png]]");
    }

    #[test]
    fn markdown_snippet_builds_a_plain_file_link() {
        assert_eq!(
            markdown_snippet("report.pdf", "report.pdf", false),
            "[report.pdf](report.pdf)"
        );
    }

    #[test]
    fn markdown_snippet_image_embed_needs_no_escaping_for_a_space() {
        assert_eq!(
            markdown_snippet("Screenshot (2).png", "Screenshot (2).png", true),
            "![[Screenshot (2).png]]"
        );
    }

    #[test]
    fn markdown_snippet_file_link_wraps_a_destination_with_a_space_in_angle_brackets() {
        assert_eq!(
            markdown_snippet("Screenshot (2).pdf", "Screenshot (2).pdf", false),
            "[Screenshot (2).pdf](<Screenshot (2).pdf>)"
        );
    }

    #[test]
    fn markdown_snippet_file_link_wraps_a_destination_with_parens_even_without_a_space() {
        assert_eq!(
            markdown_snippet("doc(1).pdf", "doc(1).pdf", false),
            "[doc(1).pdf](<doc(1).pdf>)"
        );
    }

    #[test]
    fn relative_markdown_path_in_the_same_directory_is_just_the_filename() {
        assert_eq!(
            relative_markdown_path(
                Path::new("/vault/Chapters"),
                Path::new("/vault/Chapters/pic.png")
            ),
            "pic.png"
        );
    }

    #[test]
    fn relative_markdown_path_descends_into_a_sibling_subdirectory() {
        assert_eq!(
            relative_markdown_path(
                Path::new("/vault/Chapters"),
                Path::new("/vault/Chapters/Images/pic.png")
            ),
            "Images/pic.png"
        );
    }

    #[test]
    fn relative_markdown_path_walks_up_to_a_different_subtree() {
        assert_eq!(
            relative_markdown_path(
                Path::new("/vault/Chapters"),
                Path::new("/vault/Attachments/pic.png")
            ),
            "../Attachments/pic.png"
        );
    }

    #[test]
    fn relative_markdown_path_walks_up_multiple_levels() {
        assert_eq!(
            relative_markdown_path(
                Path::new("/vault/Chapters/Act1"),
                Path::new("/vault/Attachments/pic.png")
            ),
            "../../Attachments/pic.png"
        );
    }

    #[test]
    fn suggested_clipboard_image_name_is_deterministic() {
        use chrono::TimeZone;
        let now = chrono::Local
            .with_ymd_and_hms(2026, 3, 5, 14, 30, 7)
            .unwrap();
        assert_eq!(
            suggested_clipboard_image_name(now),
            "pasted-image-20260305-143007.png"
        );
    }

    #[test]
    fn pasted_text_as_file_path_rejects_multiline_text() {
        assert_eq!(pasted_text_as_file_path("line one\nline two"), None);
    }

    #[test]
    fn pasted_text_as_file_path_rejects_a_nonexistent_path() {
        assert_eq!(pasted_text_as_file_path("/does/not/exist.png"), None);
    }

    #[test]
    fn pasted_text_as_file_path_rejects_a_relative_path() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pic.png"), b"").unwrap();
        assert_eq!(pasted_text_as_file_path("pic.png"), None);
    }

    #[test]
    fn pasted_text_as_file_path_accepts_a_bare_absolute_path() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("pic.png");
        std::fs::write(&file, b"").unwrap();
        assert_eq!(pasted_text_as_file_path(file.to_str().unwrap()), Some(file));
    }

    #[test]
    fn pasted_text_as_file_path_accepts_a_file_uri_with_percent_encoded_spaces() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("My File.png");
        std::fs::write(&file, b"").unwrap();
        let uri = format!("file://{}", file.to_str().unwrap().replace(' ', "%20"));
        assert_eq!(pasted_text_as_file_path(&uri), Some(file));
    }

    #[test]
    fn pasted_text_as_file_path_trims_surrounding_whitespace() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("pic.png");
        std::fs::write(&file, b"").unwrap();
        let padded = format!("  {}  ", file.to_str().unwrap());
        assert_eq!(pasted_text_as_file_path(&padded), Some(file));
    }

    #[test]
    fn save_attachment_creates_the_destination_directory_and_dedupes_names() {
        use crate::project::store::NativeStore;

        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("Attachments");
        let doc_dir = dir.path().join("Chapters");
        std::fs::create_dir(&doc_dir).unwrap();

        let first = save_attachment(&NativeStore, &dest, &doc_dir, "pic.png", b"one").unwrap();
        assert_eq!(first.path, dest.join("pic.png"));
        assert_eq!(first.snippet, "![[../Attachments/pic.png]]");

        let second = save_attachment(&NativeStore, &dest, &doc_dir, "pic.png", b"two").unwrap();
        assert_eq!(second.path, dest.join("pic (2).png"));
        assert_eq!(second.snippet, "![[../Attachments/pic (2).png]]");
    }

    #[test]
    fn save_attachment_links_a_non_image_file_by_name() {
        use crate::project::store::NativeStore;

        let dir = tempfile::tempdir().unwrap();
        let doc_dir = dir.path().join("Chapters");
        std::fs::create_dir(&doc_dir).unwrap();

        let saved =
            save_attachment(&NativeStore, &doc_dir, &doc_dir, "report.pdf", b"pdf").unwrap();
        assert_eq!(saved.snippet, "[report.pdf](report.pdf)");
    }
}
