use crate::project::TagGroup;

/// One node in the tag hierarchy built by [`group_tags_hierarchically`] from a flat,
/// alphabetically-sorted `[TagGroup]` list — grouping nested `#parent/child` tags the
/// way Obsidian's tag pane does, purely for display. `segment` is this node's own
/// `/`-delimited piece of the path (e.g. `"tachylite"`, not the full
/// `"projects/tachylite"`); `group` is the tag's own data (documents sharing it) when
/// a tag with this exact full path exists, or `None` for a pure grouping node — e.g.
/// `#projects` shown as a parent bucket for `#projects/tachylite` when `#projects`
/// itself was never used as a tag on any document.
struct TagNode<'a> {
    segment: String,
    group: Option<&'a TagGroup>,
    children: Vec<TagNode<'a>>,
}

/// Group a flat list of tags into a tree by splitting each on `/` (Obsidian-style
/// nesting, matching `markdown::is_tag_char`'s allowance of `/` in a tag name) —
/// doesn't change what a tag *is* or how it matches (`TagGroup`/`Project::documents_with_tag`
/// are untouched), just how a list of them is grouped for display. Segments are
/// matched case-insensitively when deciding whether two tags share a parent bucket
/// (`#Projects/A` and `#projects/B` nest under one `Projects` node, keeping whichever
/// casing was seen first — the same convention `Project::all_tags` uses for the tags
/// themselves), and children are visited in `tags`' own order, so a caller that's
/// already sorted the input (as `Project::related_by_tag` does) gets an
/// alphabetically-ordered tree back.
fn group_tags_hierarchically(tags: &[TagGroup]) -> Vec<TagNode<'_>> {
    let mut roots: Vec<TagNode> = Vec::new();
    for group in tags {
        let mut siblings = &mut roots;
        let segments: Vec<&str> = group.tag.split('/').collect();
        let last = segments.len() - 1;
        for (i, segment) in segments.into_iter().enumerate() {
            let idx = match siblings
                .iter()
                .position(|node| node.segment.eq_ignore_ascii_case(segment))
            {
                Some(idx) => idx,
                None => {
                    siblings.push(TagNode {
                        segment: segment.to_string(),
                        group: None,
                        children: Vec::new(),
                    });
                    siblings.len() - 1
                }
            };
            if i == last {
                siblings[idx].group = Some(group);
            }
            siblings = &mut siblings[idx].children;
        }
    }
    roots
}

/// Outcomes of user interaction with the Tags panel, handled by the caller
/// (`app.rs`) rather than mutated here — keeps this module a pure rendering
/// layer, matching `BacklinksEvent`. Clicking a tag heading or editing the
/// search box doesn't need an event: both just mutate `search_text` directly,
/// the same way `metadata_panel::show` mutates its draft in place.
pub enum TagsEvent {
    OpenDocument(std::path::PathBuf),
    /// The manual "Refresh" button was clicked — recompute now regardless of
    /// whether the open document has actually changed since the last scan.
    Refresh,
    /// The "Rename…" button next to a tag heading was clicked — the caller
    /// (`app.rs`) opens a name-prompt modal pre-filled with this tag, then
    /// applies it project-wide via `Project::rename_tag`.
    RenameTag(String),
}

/// Renders the Tags dock: by default, the currently-open document's own tags
/// (`tags`, frontmatter `tags:` merged with inline `#tag` mentions — see
/// `Project::related_by_tag`), each paired with the other documents in the
/// project that share it; typing into the search box (or clicking one of
/// those tag headings, which fills it in) switches to a flat, vault-wide list
/// of every document carrying the typed tag (`search_results` — see
/// `Project::documents_with_tag`). `open_path` is only used to distinguish
/// "no document open" from "document open, zero tags" — `tags` itself is
/// already scoped to whatever document is open by the caller (see `app.rs`'s
/// `recompute_tags`); this module never calls into `Project` itself.
pub fn show(
    ui: &mut egui::Ui,
    open_path: Option<&std::path::Path>,
    tags: &[TagGroup],
    search_text: &mut String,
    search_results: &[(std::path::PathBuf, String)],
) -> Option<TagsEvent> {
    let mut event = None;

    ui.horizontal(|ui| {
        ui.heading("Tags");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("Refresh").clicked() {
                event = Some(TagsEvent::Refresh);
            }
        });
    });
    ui.separator();

    ui.horizontal(|ui| {
        ui.label("Search:");
        ui.text_edit_singleline(search_text);
        if ui.small_button("Clear").clicked() {
            search_text.clear();
        }
    });
    ui.separator();

    egui::ScrollArea::vertical().show(ui, |ui| {
        if !search_text.trim().is_empty() {
            ui.label(format!("Documents tagged #{}", search_text.trim()));
            ui.add_space(4.0);
            if search_results.is_empty() {
                ui.label("No documents have this tag.");
            } else {
                for (path, title) in search_results {
                    if ui.link(title).clicked() {
                        event = Some(TagsEvent::OpenDocument(path.clone()));
                    }
                }
            }
            return;
        }

        if open_path.is_none() {
            ui.label("Open a document to see its tags.");
            return;
        }
        if tags.is_empty() {
            ui.label("This document has no tags yet.");
            return;
        }

        for node in &group_tags_hierarchically(tags) {
            render_tag_node(ui, node, search_text, &mut event);
        }
    });

    event
}

/// Render one tag's own row: its clickable `#tag` heading (fills `search_text` with
/// the tag's full path, not just this node's own segment), a "Rename…" button, and
/// the list of other documents sharing it.
fn render_tag_group(
    ui: &mut egui::Ui,
    group: &TagGroup,
    search_text: &mut String,
    event: &mut Option<TagsEvent>,
) {
    ui.horizontal(|ui| {
        if ui.link(format!("#{}", group.tag)).clicked() {
            *search_text = group.tag.clone();
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("Rename…").clicked() {
                *event = Some(TagsEvent::RenameTag(group.tag.clone()));
            }
        });
    });
    if group.documents.is_empty() {
        ui.label(egui::RichText::new("No other document shares this tag yet.").weak());
    } else {
        for (path, title) in &group.documents {
            if ui.link(title).clicked() {
                *event = Some(TagsEvent::OpenDocument(path.clone()));
            }
        }
    }
    ui.add_space(6.0);
}

/// Render one node of the tree `group_tags_hierarchically` built: a leaf (no
/// children) renders as a plain tag row; a node with children — whether or not it's
/// also a real tag in its own right — becomes a collapsible section (open by
/// default, matching Obsidian's tag pane) with its own row (if any) followed by its
/// children, each recursively rendered the same way.
fn render_tag_node(
    ui: &mut egui::Ui,
    node: &TagNode,
    search_text: &mut String,
    event: &mut Option<TagsEvent>,
) {
    if node.children.is_empty() {
        if let Some(group) = node.group {
            render_tag_group(ui, group, search_text, event);
        }
        return;
    }
    egui::CollapsingHeader::new(format!("#{}", node.segment))
        .default_open(true)
        .id_salt(&node.segment)
        .show(ui, |ui| {
            if let Some(group) = node.group {
                render_tag_group(ui, group, search_text, event);
            }
            for child in &node.children {
                render_tag_node(ui, child, search_text, event);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(tag: &str) -> TagGroup {
        TagGroup {
            tag: tag.to_string(),
            documents: Vec::new(),
        }
    }

    /// Walks `nodes` depth-first, collecting `(depth, segment, has_own_tag)` in
    /// render order — a flat, easy-to-assert-on shape for tests instead of
    /// comparing nested `TagNode`s field-by-field.
    fn flatten(nodes: &[TagNode], depth: usize, out: &mut Vec<(usize, String, bool)>) {
        for node in nodes {
            out.push((depth, node.segment.clone(), node.group.is_some()));
            flatten(&node.children, depth + 1, out);
        }
    }

    #[test]
    fn flat_tags_with_no_slash_stay_flat() {
        let tags = [group("fiction"), group("worldbuilding")];
        let tree = group_tags_hierarchically(&tags);

        let mut flat = Vec::new();
        flatten(&tree, 0, &mut flat);
        assert_eq!(
            flat,
            vec![
                (0, "fiction".to_string(), true),
                (0, "worldbuilding".to_string(), true),
            ]
        );
    }

    #[test]
    fn a_nested_tag_is_grouped_under_its_parent_segment() {
        let tags = [group("projects/tachylite")];
        let tree = group_tags_hierarchically(&tags);

        let mut flat = Vec::new();
        flatten(&tree, 0, &mut flat);
        assert_eq!(
            flat,
            vec![
                (0, "projects".to_string(), false),
                (1, "tachylite".to_string(), true),
            ]
        );
    }

    #[test]
    fn siblings_under_the_same_parent_share_one_grouping_node() {
        let tags = [group("projects/tachylite"), group("projects/smaragd")];
        let tree = group_tags_hierarchically(&tags);

        assert_eq!(tree.len(), 1, "expected one shared parent node");
        assert_eq!(tree[0].segment, "projects");
        assert!(tree[0].group.is_none());
        assert_eq!(tree[0].children.len(), 2);
        assert_eq!(tree[0].children[0].segment, "tachylite");
        assert_eq!(tree[0].children[1].segment, "smaragd");
    }

    #[test]
    fn a_parent_that_is_also_its_own_tag_keeps_its_own_data_and_its_children() {
        let tags = [group("projects"), group("projects/tachylite")];
        let tree = group_tags_hierarchically(&tags);

        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].segment, "projects");
        assert!(tree[0].group.is_some());
        assert_eq!(tree[0].group.unwrap().tag, "projects");
        assert_eq!(tree[0].children.len(), 1);
        assert_eq!(tree[0].children[0].segment, "tachylite");
    }

    #[test]
    fn parent_segments_are_matched_case_insensitively() {
        let tags = [group("Projects/A"), group("projects/B")];
        let tree = group_tags_hierarchically(&tags);

        assert_eq!(tree.len(), 1, "expected Projects/projects to share a node");
        assert_eq!(tree[0].segment, "Projects", "keeps the first-seen casing");
        assert_eq!(tree[0].children.len(), 2);
    }

    #[test]
    fn a_three_level_nested_tag_produces_a_three_level_chain() {
        let tags = [group("a/b/c")];
        let tree = group_tags_hierarchically(&tags);

        let mut flat = Vec::new();
        flatten(&tree, 0, &mut flat);
        assert_eq!(
            flat,
            vec![
                (0, "a".to_string(), false),
                (1, "b".to_string(), false),
                (2, "c".to_string(), true),
            ]
        );
    }

    #[test]
    fn empty_input_produces_an_empty_tree() {
        assert!(group_tags_hierarchically(&[]).is_empty());
    }
}
