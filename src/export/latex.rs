//! LaTeX export (see issue #126): generates `.tex` source, plus a sibling
//! `images/` directory for any referenced images and a sibling `fonts/`
//! directory for any bundled font family the style names (see
//! [`BundledFontFamily`]), for the user's own XeLaTeX/LuaLaTeX distribution
//! to compile — unlike `export::pdf`, there's no pure-Rust LaTeX engine to
//! embed, so this ships source rather than a finished file. Structured the
//! same way as `export::pdf`: `Block`/`Span` is walked into generated markup
//! text, this time targeting LaTeX instead of Typst.
//!
//! A few deliberate simplifications versus `export::pdf`, all because LaTeX
//! gives cheaper/safer ways to get a reasonable result without fighting the
//! format, not because they're impossible:
//!
//! - **Heading font/size** (`TypesetStyle::headings`) isn't mapped at all —
//!   `\chapter*`/`\section*`/etc. use the document class's own built-in,
//!   well-tested relative sizing. Reliably overriding that per level would
//!   need the `titlesec` package and careful interaction with its own
//!   internal `\normalfont` reset, which isn't something this generator can
//!   verify without a real LaTeX toolchain to compile against. Body text
//!   and blockquote/code/verse *do* get their styled font/size (via
//!   `fontspec`'s `\fontspec`/`\fontsize`), since those don't fight a
//!   built-in command's own formatting the way sectioning commands do.
//! - **Wikilinks and external links render as plain text**, same as
//!   `export::pdf` — neither resolves them to a real cross-reference; only
//!   `export::epub` currently does that, and only for wikilinks.
//! - **Drop caps** use the near-universal `lettrine` package, which handles
//!   the wrap-around-the-cap layout itself — much simpler than
//!   `export::pdf`'s hand-rolled greedy word-measuring Typst code.
//! - **The images and fonts directories are always named `images/`/`fonts/`**,
//!   not derived from the chosen output filename — keeps the generated
//!   source's `\includegraphics`/`fontspec` paths independent of the
//!   save-dialog's path, so `export_latex_string` stays pure/filesystem-free,
//!   same split as `export::pdf::export_pdf`/`export_pdf_bytes`.
//! - **Bold/italic shapes**: a style naming a [`BundledFontFamily`] (just
//!   "Libertinus Serif" today) gets real bold/italic/bold-italic `.otf`
//!   files shipped in `fonts/` and selected via `fontspec`'s `Path`/`*Font`
//!   options — no dependency on the user's system having that family
//!   installed. Any other font name (a system font, or a custom style) falls
//!   back to `AutoFakeBold`/`AutoFakeSlant` synthesis instead of fontspec's
//!   own default of silently rendering **bold**/*italic* spans as plain text
//!   when it can't find a real bold/italic shape for that family.
//!
//! Out of scope entirely: footnotes. The markdown parser has no footnote
//! concept at all (a gap shared by every export format, not new here).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use super::resolve_image_fs_path;
use super::style::TypesetStyle;
use super::{BookMeta, ExportDoc, ExportError};
use crate::markdown::{Block, BlockKind, ColumnAlignment, Span};

const LIBERTINUS_SERIF_BOLD: &[u8] = include_bytes!("../../assets/fonts/LibertinusSerif-Bold.otf");
const LIBERTINUS_SERIF_ITALIC: &[u8] =
    include_bytes!("../../assets/fonts/LibertinusSerif-Italic.otf");
const LIBERTINUS_SERIF_BOLD_ITALIC: &[u8] =
    include_bytes!("../../assets/fonts/LibertinusSerif-BoldItalic.otf");
const DEJAVU_SANS_MONO_BOLD: &[u8] = include_bytes!("../../assets/fonts/DejaVuSansMono-Bold.ttf");
const DEJAVU_SANS_MONO_ITALIC: &[u8] =
    include_bytes!("../../assets/fonts/DejaVuSansMono-Italic.ttf");
const DEJAVU_SANS_MONO_BOLD_ITALIC: &[u8] =
    include_bytes!("../../assets/fonts/DejaVuSansMono-BoldItalic.ttf");

/// A font family LaTeX export ships its own copy of (all four weights —
/// see `assets/fonts/NOTICE`), so a `\setmainfont`/`\fontspec` naming it can
/// select real bold/italic shapes via `fontspec`'s `Path`/`*Font` options
/// instead of depending on the user's system having that family installed
/// with those weights — the same self-containment guarantee
/// `export::pdf::export_pdf_bytes` already has for its embedded fonts, just
/// achieved differently since a `.tex` file can't embed font bytes inline:
/// the four files are written into a sibling `fonts/` directory instead (see
/// [`export_latex`]), the same way referenced images get a sibling `images/`.
/// `DejaVu Sans Mono`'s code/verse spans never request bold/italic (see
/// `append_span_latex`'s `span.code` branch and the `Verse` block, neither of
/// which emits `\textbf`/`\textit`), but it's still routed through here —
/// not just a plain by-name `\fontspec` — so a code block doesn't depend on
/// the user's system having "DejaVu Sans Mono" installed at all, the same
/// guarantee the main body font gets.
#[derive(Clone, Copy)]
pub struct BundledFontFamily {
    /// The filename stem fontspec's `*` wildcard substitutes when resolving
    /// `UprightFont`/`BoldFont`/etc — the four weights are always written
    /// into `fonts/` as `<stem>-Regular.<extension>` etc by [`export_latex`],
    /// regardless of what the source files under `assets/fonts/` are named.
    stem: &'static str,
    /// Including the leading dot, e.g. `".otf"`.
    extension: &'static str,
    regular: &'static [u8],
    bold: &'static [u8],
    italic: &'static [u8],
    bold_italic: &'static [u8],
}

/// The bundled family `font_name` names, if any — the default body/
/// blockquote/verse font ("Libertinus Serif") and code font ("DejaVu Sans
/// Mono") of every built-in style.
fn bundled_font_family(font_name: &str) -> Option<BundledFontFamily> {
    match font_name {
        "Libertinus Serif" => Some(BundledFontFamily {
            stem: "LibertinusSerif",
            extension: ".otf",
            regular: crate::editor_font::LIBERTINUS_SERIF,
            bold: LIBERTINUS_SERIF_BOLD,
            italic: LIBERTINUS_SERIF_ITALIC,
            bold_italic: LIBERTINUS_SERIF_BOLD_ITALIC,
        }),
        "DejaVu Sans Mono" => Some(BundledFontFamily {
            stem: "DejaVuSansMono",
            extension: ".ttf",
            regular: crate::editor_font::DEJAVU_SANS_MONO,
            bold: DEJAVU_SANS_MONO_BOLD,
            italic: DEJAVU_SANS_MONO_ITALIC,
            bold_italic: DEJAVU_SANS_MONO_BOLD_ITALIC,
        }),
        _ => None,
    }
}

/// A `\setmainfont{...}[...]`/`\fontspec{...}[...]` declaration (`cmd` is
/// `"setmainfont"`/`"fontspec"` without the backslash) for `font_name`: real
/// bundled bold/italic shapes via `Path`/`*Font` options when `font_name`
/// matches [`bundled_font_family`], otherwise a plain by-name declaration
/// (relying on the user's own system font search, same as before this
/// existed) with `AutoFakeBold`/`AutoFakeSlant` so a **bold**/*italic* span
/// still renders as *something* instead of fontspec's own default of
/// silently falling back to plain regular when no real bold/italic shape is
/// found — matching the synthesis Typst itself applies by default for
/// `export::pdf`.
fn fontspec_decl(cmd: &str, font_name: &str) -> String {
    match bundled_font_family(font_name) {
        Some(family) => format!(
            "\\{cmd}{{{}}}[Path=fonts/,Extension={},UprightFont=*-Regular,\
             BoldFont=*-Bold,ItalicFont=*-Italic,BoldItalicFont=*-BoldItalic]",
            family.stem, family.extension,
        ),
        None => format!(
            "\\{cmd}{{{}}}[AutoFakeBold=2.5,AutoFakeSlant=0.2]",
            escape_latex(font_name),
        ),
    }
}

/// Every distinct bundled family `style` actually names, across every slot
/// [`fontspec_decl`] is called for (`body`/`blockquote`/`code`/`verse`) —
/// what [`export_latex`] writes into the exported `fonts/` directory, so a
/// style that never names a bundled family (an all-system-font custom style)
/// gets no `fonts/` at all, same as an image-free manuscript getting no
/// `images/`.
fn bundled_fonts_used(style: &TypesetStyle) -> Vec<BundledFontFamily> {
    let mut found = Vec::new();
    let font_names = [
        &style.body.font,
        &style.blockquote.font,
        &style.code.font,
        &style.verse.font,
    ];
    for font_name in font_names {
        if let Some(family) = bundled_font_family(font_name) {
            if !found.iter().any(|f: &BundledFontFamily| f.stem == family.stem) {
                found.push(family);
            }
        }
    }
    found
}

/// [`export_latex_string`], written to `out_path` plus a sibling `images/`
/// directory holding a copy of every image the manuscript references
/// (created only if there's at least one) — the native save-dialog path.
/// Returns how many images were copied, for the caller's status message.
pub fn export_latex(
    docs: &[ExportDoc],
    meta: &BookMeta,
    style: &TypesetStyle,
    project_root: &Path,
    out_path: &Path,
) -> Result<usize, ExportError> {
    let (source, images, fonts) = export_latex_string(docs, meta, style, project_root);
    fs::write(out_path, source)?;
    let out_dir = out_path.parent().unwrap_or_else(|| Path::new("."));
    if !fonts.is_empty() {
        let fonts_dir = out_dir.join("fonts");
        fs::create_dir_all(&fonts_dir)?;
        for family in &fonts {
            let ext = family.extension;
            let stem = family.stem;
            fs::write(fonts_dir.join(format!("{stem}-Regular{ext}")), family.regular)?;
            fs::write(fonts_dir.join(format!("{stem}-Bold{ext}")), family.bold)?;
            fs::write(fonts_dir.join(format!("{stem}-Italic{ext}")), family.italic)?;
            fs::write(fonts_dir.join(format!("{stem}-BoldItalic{ext}")), family.bold_italic)?;
        }
    }
    if images.is_empty() {
        return Ok(0);
    }
    let images_dir = out_dir.join("images");
    fs::create_dir_all(&images_dir)?;
    for (source_path, dest_name) in &images {
        fs::copy(source_path, images_dir.join(dest_name))?;
    }
    Ok(images.len())
}

/// Renders `docs` to LaTeX source, pure and filesystem-free — returns the
/// generated source plus the (resolved source path, assigned filename within
/// `images/`) pairs the caller should copy. Kept separate from
/// [`export_latex`] so markup generation is independently testable, the same
/// split `export::pdf::export_pdf`/`export_pdf_bytes` uses.
pub fn export_latex_string(
    docs: &[ExportDoc],
    meta: &BookMeta,
    style: &TypesetStyle,
    project_root: &Path,
) -> (String, Vec<(PathBuf, String)>, Vec<BundledFontFamily>) {
    let mut images = ImageCollector::default();
    let mut source = generate_preamble(meta, style);
    let fonts = bundled_fonts_used(style);
    source.push_str("\\begin{document}\n");
    let leading = style.body.size_pt as f32 * style.body.line_height;
    source.push_str(&format!(
        "\\fontsize{{{}pt}}{{{leading}pt}}\\selectfont\n",
        style.body.size_pt
    ));
    source.push_str(if style.body.justify {
        "\\justifying\n"
    } else {
        "\\RaggedRight\n"
    });
    source.push_str(&title_page_latex(meta, style));
    for doc in docs {
        source.push_str("\\clearpage\n");
        let doc_dir = doc.source_path.parent().unwrap_or(project_root);
        source.push_str(&blocks_to_latex(
            doc,
            doc_dir,
            project_root,
            style,
            &mut images,
        ));
    }
    source.push_str("\\end{document}\n");
    (source, images.copies, fonts)
}

fn generate_preamble(meta: &BookMeta, style: &TypesetStyle) -> String {
    let mut s = String::new();
    s.push_str(
        "% Requires XeLaTeX or LuaLaTeX (for fontspec and Unicode support) \
         — will not compile with plain pdflatex.\n",
    );
    s.push_str("\\documentclass[12pt]{book}\n");
    s.push_str(&format!(
        "\\usepackage[paperwidth={}mm,paperheight={}mm,margin={}mm]{{geometry}}\n",
        style.page.width_mm, style.page.height_mm, style.page.margin_mm
    ));
    s.push_str("\\usepackage{fontspec}\n");
    s.push_str(&fontspec_decl("setmainfont", &style.body.font));
    s.push('\n');
    s.push_str("\\usepackage{graphicx}\n");
    s.push_str("\\usepackage{verse}\n");
    s.push_str("\\usepackage[normalem]{ulem}\n");
    s.push_str("\\usepackage{ragged2e}\n");
    if style.drop_cap.is_some() {
        s.push_str("\\usepackage{lettrine}\n");
    }
    match &style.running_header {
        Some(rh) => {
            s.push_str("\\usepackage{fancyhdr}\n\\pagestyle{fancy}\n\\fancyhf{}\n");
            s.push_str(&format!(
                "\\lhead{{{}}}\n",
                header_side_latex(&rh.left, meta, "\\leftmark")
            ));
            s.push_str(&format!(
                "\\rhead{{{}}}\n",
                header_side_latex(&rh.right, meta, "\\rightmark")
            ));
            s.push_str("\\cfoot{\\thepage}\n");
        }
        None => s.push_str("\\pagestyle{plain}\n"),
    }
    s
}

/// A running-header slot's generated LaTeX: `{chapter}` (and only exactly
/// that, not mixed with other text — a v1 simplification, same one
/// `export::pdf::header_side` makes) uses `mark_cmd` (`\leftmark`/
/// `\rightmark`, kept in sync with the current chapter title by the explicit
/// `\markboth` call `blocks_to_latex` emits per document — not relying on
/// `\chapter*`'s own automatic mark, which starred sectioning commands don't
/// trigger); anything else is `{title}`/`{subtitle}`/`{author}`-substituted
/// literal text.
fn header_side_latex(template: &str, meta: &BookMeta, mark_cmd: &str) -> String {
    if template.trim() == "{chapter}" {
        mark_cmd.to_string()
    } else {
        escape_latex(
            &template
                .replace("{title}", &meta.title)
                .replace("{subtitle}", &meta.subtitle)
                .replace("{author}", &meta.author),
        )
    }
}

/// A centered Title/Subtitle/Author page — mirrors `export::pdf::title_page_typst`.
/// Empty when none of the three fields are set.
fn title_page_latex(meta: &BookMeta, style: &TypesetStyle) -> String {
    if meta.title.is_empty() && meta.subtitle.is_empty() && meta.author.is_empty() {
        return String::new();
    }
    let mut s = String::new();
    let _ = style;
    s.push_str("\\begin{titlepage}\n\\centering\n");
    if !meta.title.is_empty() {
        s.push_str(&format!(
            "\\vspace*{{4cm}}\n{{\\Huge\\bfseries {}}}\\par\n",
            escape_latex(&meta.title)
        ));
    }
    if !meta.subtitle.is_empty() {
        s.push_str(&format!(
            "\\vspace{{0.8em}}\n{{\\Large\\itshape {}}}\\par\n",
            escape_latex(&meta.subtitle)
        ));
    }
    if !meta.author.is_empty() {
        s.push_str(&format!(
            "\\vspace{{2em}}\n{{\\large {}}}\\par\n",
            escape_latex(&meta.author)
        ));
    }
    s.push_str("\\end{titlepage}\n");
    s
}

/// One `ExportDoc` as an unnumbered chapter — mirrors `export::pdf::blocks_to_typst`.
fn blocks_to_latex(
    doc: &ExportDoc,
    doc_dir: &Path,
    project_root: &Path,
    style: &TypesetStyle,
    images: &mut ImageCollector,
) -> String {
    let mut out = String::new();
    let title = escape_latex(&doc.title);
    // `\chapter*` (unnumbered, matching Typst headings having no automatic
    // numbering either) doesn't trigger LaTeX's usual running-head mark —
    // only the unstarred form does — so the mark is set explicitly here
    // instead of relying on `\chaptermark`.
    out.push_str(&format!("\\chapter*{{{title}}}\n\\markboth{{{title}}}{{{title}}}\n"));
    let mut first_paragraph_seen = false;
    let mut list_stack: Vec<bool> = Vec::new();
    for block in &doc.blocks {
        if !matches!(block.kind, BlockKind::ListItem { .. }) {
            close_lists(&mut out, &mut list_stack);
        }
        let drop_cap_here = style.drop_cap.is_some()
            && !first_paragraph_seen
            && matches!(block.kind, BlockKind::Paragraph);
        if matches!(block.kind, BlockKind::Paragraph) {
            first_paragraph_seen = true;
        }
        append_latex_block(
            &mut out,
            block,
            drop_cap_here,
            style,
            doc_dir,
            project_root,
            images,
            &mut list_stack,
        );
    }
    close_lists(&mut out, &mut list_stack);
    out
}

/// Closes every still-open `itemize`/`enumerate` environment — called
/// whenever the block stream leaves list context (the next block isn't a
/// `ListItem`) or a document ends, so a manuscript's lists always balance
/// their own `\begin`/`\end` pairs.
fn close_lists(out: &mut String, stack: &mut Vec<bool>) {
    while let Some(ordered) = stack.pop() {
        out.push_str(if ordered {
            "\\end{enumerate}\n"
        } else {
            "\\end{itemize}\n"
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn append_latex_block(
    out: &mut String,
    block: &Block,
    drop_cap_here: bool,
    style: &TypesetStyle,
    doc_dir: &Path,
    project_root: &Path,
    images: &mut ImageCollector,
    list_stack: &mut Vec<bool>,
) {
    match &block.kind {
        BlockKind::Heading(level) => {
            // Shifted one level deeper, same reasoning as
            // `export::pdf::append_typst_block`: level 1 is reserved for the
            // synthetic per-document `\chapter*` title, so a document's own
            // `# H1` never collides with it.
            let level = ((*level as usize) + 1).min(6);
            let cmd = match level {
                1 => "section",
                2 => "subsection",
                3 => "subsubsection",
                4 => "paragraph",
                _ => "subparagraph",
            };
            out.push_str(&format!("\\{cmd}*{{"));
            spans_to_latex(out, &block.spans, doc_dir, project_root, images);
            out.push_str("}\n\n");
        }
        BlockKind::Paragraph => {
            if drop_cap_here && style.drop_cap.is_some() {
                append_paragraph_with_drop_cap(out, &block.spans, style, doc_dir, project_root, images);
            } else {
                spans_to_latex(out, &block.spans, doc_dir, project_root, images);
            }
            out.push_str("\n\n");
        }
        BlockKind::CodeBlock { .. } => {
            let text: String = block.spans.iter().map(|s| s.text.as_str()).collect();
            // `verbatim` has no escape mechanism of its own; guard against the
            // one sequence that would prematurely end the environment, same
            // spirit as `export::pdf` guarding against a literal ` ``` `.
            let safe = text.replace("\\end{verbatim}", "\\end {verbatim}");
            out.push_str(&format!(
                "{{{}\\fontsize{{{}pt}}{{{}pt}}\\selectfont\n\\begin{{verbatim}}\n{safe}\n\\end{{verbatim}}\n}}\n\n",
                fontspec_decl("fontspec", &style.code.font),
                style.code.size_pt,
                style.code.size_pt as f32 * 1.2,
            ));
        }
        BlockKind::Verse => {
            let text: String = block.spans.iter().map(|s| s.text.as_str()).collect();
            let lines: Vec<String> = text
                .trim_end_matches('\n')
                .split('\n')
                .map(escape_latex)
                .collect();
            out.push_str(&format!(
                "{{{}\\fontsize{{{}pt}}{{{}pt}}\\selectfont{}\n\\begin{{verse}}\n{}\n\\end{{verse}}\n}}\n\n",
                fontspec_decl("fontspec", &style.verse.font),
                style.verse.size_pt,
                style.verse.size_pt as f32 * 1.2,
                if style.verse.italic { "\\itshape" } else { "" },
                lines.join(" \\\\\n"),
            ));
        }
        BlockKind::BlockQuote => {
            out.push_str(&format!(
                "\\begin{{quote}}\n{{{}\\fontsize{{{}pt}}{{{}pt}}\\selectfont{}",
                fontspec_decl("fontspec", &style.blockquote.font),
                style.blockquote.size_pt,
                style.blockquote.size_pt as f32 * 1.2,
                if style.blockquote.italic { "\\itshape " } else { "" },
            ));
            spans_to_latex(out, &block.spans, doc_dir, project_root, images);
            out.push_str("}\n\\end{quote}\n\n");
        }
        BlockKind::ListItem { ordered, depth, .. } => {
            let depth = *depth as usize;
            while list_stack.len() > depth + 1 {
                let o = list_stack.pop().unwrap();
                out.push_str(if o { "\\end{enumerate}\n" } else { "\\end{itemize}\n" });
            }
            if list_stack.len() == depth + 1 && list_stack[depth] != *ordered {
                list_stack.pop();
                out.push_str(if *ordered { "\\end{itemize}\n" } else { "\\end{enumerate}\n" });
            }
            while list_stack.len() <= depth {
                list_stack.push(*ordered);
                out.push_str(if *ordered { "\\begin{enumerate}\n" } else { "\\begin{itemize}\n" });
            }
            out.push_str("\\item ");
            spans_to_latex(out, &block.spans, doc_dir, project_root, images);
            out.push('\n');
        }
        BlockKind::Rule => out.push_str("\\bigskip\\centerline{*}\\bigskip\n\n"),
        BlockKind::Table {
            alignments,
            header,
            rows,
        } => {
            let columns = header.len().max(1);
            let spec: String = (0..columns)
                .map(|i| match alignments.get(i) {
                    Some(ColumnAlignment::Center) => 'c',
                    Some(ColumnAlignment::Right) => 'r',
                    _ => 'l',
                })
                .collect();
            out.push_str(&format!("\\begin{{tabular}}{{{spec}}}\n\\hline\n"));
            for (i, cell) in header.iter().enumerate() {
                if i > 0 {
                    out.push_str(" & ");
                }
                out.push_str("\\textbf{");
                spans_to_latex(out, cell, doc_dir, project_root, images);
                out.push('}');
            }
            out.push_str(" \\\\\n\\hline\n");
            for row in rows {
                for (i, cell) in row.iter().enumerate() {
                    if i > 0 {
                        out.push_str(" & ");
                    }
                    spans_to_latex(out, cell, doc_dir, project_root, images);
                }
                out.push_str(" \\\\\n");
            }
            out.push_str("\\hline\n\\end{tabular}\n\n");
        }
    }
}

/// `\lettrine`'s own layout (greedily wrapping body text next to the
/// enlarged cap) replaces `export::pdf::append_paragraph_with_drop_cap`'s
/// hand-rolled word-measuring Typst helper entirely — the package does the
/// hard part, so this just needs to split off the first letter and the rest
/// of its word for `\lettrine`'s two arguments.
fn append_paragraph_with_drop_cap(
    out: &mut String,
    spans: &[Span],
    style: &TypesetStyle,
    doc_dir: &Path,
    project_root: &Path,
    images: &mut ImageCollector,
) {
    let drop_cap = style
        .drop_cap
        .expect("caller only invokes this when style.drop_cap.is_some()");
    let Some(first_span) = spans.iter().find(|s| s.image.is_none() && !s.text.is_empty()) else {
        spans_to_latex(out, spans, doc_dir, project_root, images);
        return;
    };
    let mut chars = first_span.text.chars();
    let Some(first_char) = chars.next() else {
        spans_to_latex(out, spans, doc_dir, project_root, images);
        return;
    };
    let rest = chars.as_str();
    let word_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let rest_of_word = &rest[..word_end];
    let remainder = &rest[word_end..];

    // Same heuristic as `DropCapStyle::scale`'s doc comment describes for the
    // PDF renderer: cap height in ems ÷ line-height multiplier, not derived
    // from the font's real cap-height metric.
    let lines = ((drop_cap.scale / style.body.line_height).round() as u32).max(2);
    out.push_str(&format!(
        "\\lettrine[lines={lines}]{{{}}}{{{}}}",
        escape_latex(&first_char.to_string()),
        escape_latex(rest_of_word),
    ));
    out.push_str(&escape_latex(remainder));

    let first_span_ptr = first_span as *const Span;
    for span in spans {
        if std::ptr::eq(span, first_span_ptr) {
            continue;
        }
        append_span_latex(out, span, doc_dir, project_root, images);
    }
}

fn spans_to_latex(out: &mut String, spans: &[Span], doc_dir: &Path, project_root: &Path, images: &mut ImageCollector) {
    for span in spans {
        append_span_latex(out, span, doc_dir, project_root, images);
    }
}

fn append_span_latex(out: &mut String, span: &Span, doc_dir: &Path, project_root: &Path, images: &mut ImageCollector) {
    if let Some(image) = &span.image {
        match resolve_image_fs_path(&image.src, doc_dir, project_root) {
            Some(resolved) => {
                let name = images.assign(resolved);
                out.push_str(&format!("\\includegraphics[width=\\linewidth]{{images/{name}}}\n"));
            }
            None => {
                out.push_str(&escape_latex(&span.text));
                out.push(' ');
            }
        }
        return;
    }
    let mut text = escape_latex(&span.text);
    if span.code {
        text = format!("\\texttt{{{}}}", escape_latex(&span.text));
    } else {
        if span.bold {
            text = format!("\\textbf{{{text}}}");
        }
        if span.italic {
            text = format!("\\textit{{{text}}}");
        }
        if span.strikethrough {
            text = format!("\\sout{{{text}}}");
        }
    }
    out.push_str(&text);
    out.push(' ');
}

/// Escapes LaTeX's special characters. Not a uniform "prepend a backslash"
/// loop like `export::pdf::escape_typst`: a literal backslash needs the
/// `\textbackslash{}` command (`\\` means "line break" in LaTeX, not a
/// literal backslash), and `~`/`^` need `\textasciitilde{}`/
/// `\textasciicircum{}` specifically — a bare backslash-prefix doesn't
/// produce the literal character for either.
fn escape_latex(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\textbackslash{}"),
            '#' | '$' | '%' | '&' | '_' | '{' | '}' => {
                out.push('\\');
                out.push(ch);
            }
            '~' => out.push_str("\\textasciitilde{}"),
            '^' => out.push_str("\\textasciicircum{}"),
            _ => out.push(ch),
        }
    }
    out
}

/// Assigns a stable, deduplicated destination filename (within `images/`) to
/// each distinct resolved image path referenced across a whole export — the
/// same resolve-then-dedupe shape `export::epub::embed_epub_image` already
/// uses for its own (zip-internal, not real-filesystem) resource names.
#[derive(Default)]
struct ImageCollector {
    seen: HashMap<PathBuf, String>,
    used_names: HashSet<String>,
    copies: Vec<(PathBuf, String)>,
}

impl ImageCollector {
    fn assign(&mut self, resolved: PathBuf) -> String {
        if let Some(existing) = self.seen.get(&resolved) {
            return existing.clone();
        }
        let base = resolved
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "image".to_string());
        let (stem, ext) = match base.rsplit_once('.') {
            Some((stem, ext)) => (stem.to_string(), Some(ext.to_string())),
            None => (base.clone(), None),
        };
        let mut name = base.clone();
        let mut counter = 1;
        while self.used_names.contains(&name) {
            counter += 1;
            name = match &ext {
                Some(ext) => format!("{stem}_{counter}.{ext}"),
                None => format!("{stem}_{counter}"),
            };
        }
        self.used_names.insert(name.clone());
        self.seen.insert(resolved.clone(), name.clone());
        self.copies.push((resolved, name.clone()));
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::style::built_in_styles;
    use crate::markdown;

    fn sample_blocks() -> Vec<Block> {
        markdown::parse(
            "# Heading\n\nA *paragraph* with **bold**, `code`, and a [[Wikilink]].\n\n\
             > A quote\n\n- one\n- two\n\n1. first\n2. second\n\n---\n\n\
             ```\ncode line\n```\n\n```verse\nline one\nline two\n```\n\n\
             | a | b |\n|---|---|\n| 1 | 2 |\n",
        )
    }

    fn manuscript_style() -> TypesetStyle {
        built_in_styles().remove(0)
    }

    fn trade_paperback_style() -> TypesetStyle {
        built_in_styles().remove(1)
    }

    #[test]
    fn escape_latex_escapes_every_special_character() {
        assert_eq!(
            escape_latex("a#b$c%d&e_f{g}h~i^j\\k"),
            "a\\#b\\$c\\%d\\&e\\_f\\{g\\}h\\textasciitilde{}i\\textasciicircum{}j\\textbackslash{}k"
        );
        assert_eq!(escape_latex("plain text"), "plain text");
    }

    #[test]
    fn title_page_latex_is_empty_for_a_default_book_meta() {
        assert_eq!(title_page_latex(&BookMeta::default(), &manuscript_style()), "");
    }

    #[test]
    fn title_page_latex_includes_title_subtitle_and_author_when_set() {
        let meta = BookMeta {
            title: "My Book".to_string(),
            subtitle: "A Subtitle".to_string(),
            author: "Jane Doe".to_string(),
        };
        let page = title_page_latex(&meta, &manuscript_style());
        assert!(page.contains("My Book"));
        assert!(page.contains("A Subtitle"));
        assert!(page.contains("Jane Doe"));
    }

    #[test]
    fn header_side_latex_substitutes_the_subtitle_token() {
        let meta = BookMeta {
            title: "My Book".to_string(),
            subtitle: "A Subtitle".to_string(),
            author: "Jane Doe".to_string(),
        };
        assert_eq!(header_side_latex("{subtitle}", &meta, "\\leftmark"), "A Subtitle");
    }

    #[test]
    fn header_side_latex_uses_the_mark_command_for_the_bare_chapter_token() {
        assert_eq!(
            header_side_latex("{chapter}", &BookMeta::default(), "\\rightmark"),
            "\\rightmark"
        );
    }

    #[test]
    fn generate_preamble_includes_lettrine_only_when_the_style_uses_a_drop_cap() {
        let meta = BookMeta::default();
        assert!(generate_preamble(&meta, &trade_paperback_style()).contains("lettrine"));
        assert!(!generate_preamble(&meta, &manuscript_style()).contains("lettrine"));
    }

    #[test]
    fn generate_preamble_wires_fancyhdr_only_when_the_style_has_a_running_header() {
        let meta = BookMeta::default();
        assert!(generate_preamble(&meta, &trade_paperback_style()).contains("fancyhdr"));
        assert!(!generate_preamble(&meta, &manuscript_style()).contains("fancyhdr"));
    }

    #[test]
    fn fontspec_decl_uses_the_bundled_path_for_libertinus_serif() {
        let decl = fontspec_decl("setmainfont", "Libertinus Serif");
        assert!(decl.starts_with("\\setmainfont{LibertinusSerif}["));
        assert!(decl.contains("Path=fonts/"));
        assert!(decl.contains("BoldFont=*-Bold"));
        assert!(decl.contains("ItalicFont=*-Italic"));
        assert!(decl.contains("BoldItalicFont=*-BoldItalic"));
    }

    #[test]
    fn fontspec_decl_falls_back_to_autofakebold_for_an_unbundled_font() {
        let decl = fontspec_decl("fontspec", "Comic Sans MS");
        assert_eq!(decl, "\\fontspec{Comic Sans MS}[AutoFakeBold=2.5,AutoFakeSlant=0.2]");
    }

    #[test]
    fn generate_preamble_setmainfont_line_matches_fontspec_decl() {
        let meta = BookMeta::default();
        let preamble = generate_preamble(&meta, &manuscript_style());
        assert!(preamble.contains(&fontspec_decl("setmainfont", "Libertinus Serif")));
    }

    #[test]
    fn bundled_fonts_used_dedupes_repeated_families_and_finds_both_bundled_families() {
        // Every built-in style's body/blockquote/verse font is "Libertinus
        // Serif" (one family despite three slots naming it) and its code
        // font is "DejaVu Sans Mono" (a second, distinct family).
        let fonts = bundled_fonts_used(&manuscript_style());
        let mut stems: Vec<&str> = fonts.iter().map(|f| f.stem).collect();
        stems.sort();
        assert_eq!(stems, ["DejaVuSansMono", "LibertinusSerif"]);
    }

    #[test]
    fn export_latex_writes_a_fonts_directory_with_all_four_bundled_weights() {
        let dir = tempfile::tempdir().unwrap();
        let docs = vec![ExportDoc {
            title: "Chapter One".to_string(),
            blocks: markdown::parse("Just text."),
            source_path: dir.path().join("one.md"),
        }];
        let out = dir.path().join("out.tex");
        export_latex(&docs, &BookMeta::default(), &manuscript_style(), dir.path(), &out).unwrap();
        let fonts_dir = dir.path().join("fonts");
        for weight in ["Regular", "Bold", "Italic", "BoldItalic"] {
            for (stem, ext) in [("LibertinusSerif", "otf"), ("DejaVuSansMono", "ttf")] {
                let file = fonts_dir.join(format!("{stem}-{weight}.{ext}"));
                assert!(file.exists(), "expected {file:?} to exist");
                assert!(!fs::read(&file).unwrap().is_empty());
            }
        }
    }

    #[test]
    fn heading_block_emits_an_unnumbered_section_shifted_one_level() {
        let style = manuscript_style();
        let blocks = markdown::parse("# A Heading\n");
        let mut out = String::new();
        let mut images = ImageCollector::default();
        let mut stack = Vec::new();
        append_latex_block(&mut out, &blocks[0], false, &style, Path::new("."), Path::new("."), &mut images, &mut stack);
        // Shifted one level deeper (level 1 is reserved for the synthetic
        // per-document `\chapter*`, emitted separately by `blocks_to_latex`).
        assert!(out.contains("\\subsection*{A Heading"));
    }

    #[test]
    fn verse_block_preserves_line_breaks_with_a_latex_break() {
        let style = manuscript_style();
        let blocks = markdown::parse("```verse\nline one\nline two\n```\n");
        let mut out = String::new();
        let mut images = ImageCollector::default();
        let mut stack = Vec::new();
        append_latex_block(&mut out, &blocks[0], false, &style, Path::new("."), Path::new("."), &mut images, &mut stack);
        assert!(out.contains("\\begin{verse}"));
        assert!(
            out.contains("line one \\\\\nline two"),
            "expected a LaTeX line break between lines, got: {out}"
        );
    }

    #[test]
    fn nested_list_items_balance_their_begin_end_pairs() {
        let style = manuscript_style();
        let blocks = markdown::parse("- one\n  - nested\n- two\n");
        let mut out = String::new();
        let mut images = ImageCollector::default();
        let mut stack = Vec::new();
        for block in &blocks {
            append_latex_block(&mut out, block, false, &style, Path::new("."), Path::new("."), &mut images, &mut stack);
        }
        close_lists(&mut out, &mut stack);
        assert_eq!(out.matches("\\begin{itemize}").count(), out.matches("\\end{itemize}").count());
        assert!(stack.is_empty());
    }

    #[test]
    fn a_list_switching_from_unordered_to_ordered_at_the_same_depth_closes_and_reopens() {
        let style = manuscript_style();
        let blocks = markdown::parse("- one\n\n1. first\n");
        let mut out = String::new();
        let mut images = ImageCollector::default();
        let mut stack = Vec::new();
        for block in &blocks {
            if !matches!(block.kind, BlockKind::ListItem { .. }) {
                close_lists(&mut out, &mut stack);
            }
            append_latex_block(&mut out, block, false, &style, Path::new("."), Path::new("."), &mut images, &mut stack);
        }
        close_lists(&mut out, &mut stack);
        assert!(out.contains("\\begin{itemize}"));
        assert!(out.contains("\\end{itemize}"));
        assert!(out.contains("\\begin{enumerate}"));
        assert!(out.contains("\\end{enumerate}"));
    }

    #[test]
    fn table_block_emits_aligned_columns_and_a_bolded_header() {
        let style = manuscript_style();
        let blocks = markdown::parse("| a | b |\n|:--|--:|\n| 1 | 2 |\n");
        let mut out = String::new();
        let mut images = ImageCollector::default();
        let mut stack = Vec::new();
        append_latex_block(&mut out, &blocks[0], false, &style, Path::new("."), Path::new("."), &mut images, &mut stack);
        assert!(out.contains("\\begin{tabular}{lr}"));
        assert!(out.contains("\\textbf{a"));
    }

    #[test]
    fn image_collector_deduplicates_the_same_path_and_disambiguates_name_collisions() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("sub_a").join("cover.png");
        let b = dir.path().join("sub_b").join("cover.png");
        fs::create_dir_all(a.parent().unwrap()).unwrap();
        fs::create_dir_all(b.parent().unwrap()).unwrap();
        fs::write(&a, "a").unwrap();
        fs::write(&b, "b").unwrap();

        let mut images = ImageCollector::default();
        let name_a = images.assign(a.clone());
        let name_a_again = images.assign(a);
        let name_b = images.assign(b);

        assert_eq!(name_a, name_a_again, "the same path should get the same name twice");
        assert_ne!(name_a, name_b, "two different paths with the same filename must not collide");
        assert_eq!(images.copies.len(), 2);
    }

    #[test]
    fn export_latex_does_not_write_an_images_directory_when_there_are_no_images() {
        let dir = tempfile::tempdir().unwrap();
        let docs = vec![ExportDoc {
            title: "Chapter One".to_string(),
            blocks: sample_blocks(),
            source_path: dir.path().join("chapter1.md"),
        }];
        let meta = BookMeta {
            title: "My Book".to_string(),
            subtitle: "A Subtitle".to_string(),
            author: "Jane Doe".to_string(),
        };
        let out = dir.path().join("out.tex");
        let count = export_latex(&docs, &meta, &manuscript_style(), dir.path(), &out).unwrap();
        assert!(out.exists());
        assert_eq!(count, 0);
        assert!(!dir.path().join("images").exists());
    }

    #[test]
    fn export_latex_copies_referenced_images_into_an_images_directory() {
        let dir = tempfile::tempdir().unwrap();
        let image_path = dir.path().join("photo.png");
        fs::write(&image_path, b"fake png bytes").unwrap();
        let docs = vec![ExportDoc {
            title: "Chapter One".to_string(),
            blocks: markdown::parse("![alt](photo.png)\n"),
            source_path: dir.path().join("chapter1.md"),
        }];
        let out = dir.path().join("out.tex");
        let count = export_latex(&docs, &BookMeta::default(), &manuscript_style(), dir.path(), &out).unwrap();
        assert_eq!(count, 1);
        assert!(dir.path().join("images").join("photo.png").exists());
        let source = fs::read_to_string(&out).unwrap();
        assert!(source.contains("\\includegraphics[width=\\linewidth]{images/photo.png}"));
    }

    #[test]
    fn export_latex_with_drop_cap_and_running_header_style_does_not_panic() {
        let dir = tempfile::tempdir().unwrap();
        let docs = vec![
            ExportDoc {
                title: "Chapter One".to_string(),
                blocks: markdown::parse(
                    "First paragraph of chapter one, long enough to matter for the drop cap.\n\n\
                     Second paragraph.",
                ),
                source_path: dir.path().join("one.md"),
            },
            ExportDoc {
                title: "Chapter Two".to_string(),
                blocks: markdown::parse("First paragraph of chapter two."),
                source_path: dir.path().join("two.md"),
            },
        ];
        let meta = BookMeta {
            title: "My Book".to_string(),
            subtitle: "A Subtitle".to_string(),
            author: "Jane Doe".to_string(),
        };
        let out = dir.path().join("out.tex");
        export_latex(&docs, &meta, &trade_paperback_style(), dir.path(), &out).unwrap();
        let source = fs::read_to_string(&out).unwrap();
        assert!(source.contains("\\lettrine"));
        // trade_paperback_style's running header is `{author}` on the left,
        // `{chapter}` on the right — only the latter resolves to a mark command.
        assert!(source.contains("\\rightmark"));
    }

    #[test]
    fn export_latex_does_not_panic_on_every_block_kind() {
        let dir = tempfile::tempdir().unwrap();
        let docs = vec![ExportDoc {
            title: "Chapter One".to_string(),
            blocks: sample_blocks(),
            source_path: dir.path().join("chapter1.md"),
        }];
        let meta = BookMeta {
            title: "My Book".to_string(),
            subtitle: "A Subtitle".to_string(),
            author: "Jane Doe".to_string(),
        };
        let out = dir.path().join("out.tex");
        export_latex(&docs, &meta, &manuscript_style(), dir.path(), &out).unwrap();
        assert!(out.exists());
    }

    #[test]
    fn empty_book_meta_is_handled_without_a_title_or_author() {
        let dir = tempfile::tempdir().unwrap();
        let docs = vec![ExportDoc {
            title: "Solo".to_string(),
            blocks: markdown::parse("Just text."),
            source_path: dir.path().join("solo.md"),
        }];
        let out = dir.path().join("solo.tex");
        export_latex(&docs, &BookMeta::default(), &manuscript_style(), dir.path(), &out).unwrap();
        assert!(out.exists());
    }
}
