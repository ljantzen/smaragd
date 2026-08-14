use crate::editor_font::EditorFont;
use crate::settings::Settings;
use crate::shortcuts::{ShortcutAction, ShortcutCategory, ShortcutTarget, is_safe_binding};
use crate::spellcheck::SpellCheckLanguage;

/// Keyboard filter claimed on every focused category row in the settings nav
/// list — same mechanism `binder_panel.rs`'s `ARROW_KEYS_FILTER` uses and for
/// the same reason: without it, egui's own built-in "arrow keys move focus to
/// the nearest widget in that direction" behavior also reacts to the same
/// Up/Down press, racing with the manual handling below and leaving keyboard
/// focus (and its highlight) one row ahead of `*category` — the category the
/// content pane actually shows lagging behind the row that visibly has focus.
/// Only vertical arrows are claimed; unlike the binder, there's nothing for
/// Left/Right to do in this flat list.
const CATEGORY_ARROW_KEYS_FILTER: egui::EventFilter = egui::EventFilter {
    tab: false,
    horizontal_arrows: false,
    vertical_arrows: true,
    escape: false,
};

/// Which page of the settings window is currently showing — purely UI
/// navigation state, not persisted (unlike everything in `Settings` itself),
/// so it lives here rather than on that struct.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsCategory {
    General,
    Appearance,
    Editor,
    SpellCheck,
    Templates,
    History,
    Pomodoro,
    Shortcuts,
}

impl SettingsCategory {
    pub const ALL: [SettingsCategory; 8] = [
        SettingsCategory::General,
        SettingsCategory::Appearance,
        SettingsCategory::Editor,
        SettingsCategory::SpellCheck,
        SettingsCategory::Templates,
        SettingsCategory::History,
        SettingsCategory::Pomodoro,
        SettingsCategory::Shortcuts,
    ];

    fn label(self) -> &'static str {
        match self {
            SettingsCategory::General => "General",
            SettingsCategory::Appearance => "Appearance",
            SettingsCategory::Editor => "Editor",
            SettingsCategory::SpellCheck => "Spell Check",
            SettingsCategory::Templates => "Templates",
            SettingsCategory::History => "History",
            SettingsCategory::Pomodoro => "Pomodoro",
            SettingsCategory::Shortcuts => "Shortcuts",
        }
    }
}

/// Renders the settings dialog when `open` is true (does nothing and returns
/// `false` otherwise — unlike `egui::Window`, `egui::Modal` has no built-in
/// `.open()`, so this guards it manually), as an IntelliJ-style two-pane
/// dialog: a left-hand category list (`category`, which must persist across
/// frames the same way `recording_shortcut` below does — the caller owns both
/// alongside `settings`) and the selected category's controls on the right.
/// `recording_shortcut` tracks which binding, if any, is currently capturing its
/// next keypress (built-in or plugin — see `ShortcutTarget`). `plugin_shortcut_rows`
/// is every plugin `:` command that declared a shortcut (`register_shortcut`)
/// paired with its current effective binding, if any (`app.rs`'s
/// `compute_effective_plugin_shortcuts`) — `Settings` alone doesn't know which
/// plugins are loaded. Returns `true` if `settings` changed this frame, so the
/// caller can persist it to disk (and recompute the effective plugin shortcuts,
/// since an edit here can change which ones are free).
///
/// A real `egui::Modal`, not `egui::Window`: the window used to let clicks and
/// keystrokes reach the binder (or anything else) underneath it while open —
/// `Window` alone doesn't block input to the rest of the UI, it just floats
/// on top visually. `Modal` adds the dimmed backdrop that actually captures
/// that input, at the cost of the free resizing/dragging/title-bar chrome
/// `Window` gave for nothing; those aren't essential for a settings dialog.
#[allow(clippy::too_many_arguments)]
pub fn show(
    ctx: &egui::Context,
    open: &mut bool,
    settings: &mut Settings,
    category: &mut SettingsCategory,
    recording_shortcut: &mut Option<ShortcutTarget>,
    plugin_shortcut_rows: &[(String, Option<egui::KeyboardShortcut>)],
    dictionary_downloading: Option<SpellCheckLanguage>,
    dictionary_download_request: &mut Option<SpellCheckLanguage>,
) -> bool {
    // Detects the dialog's closed->open transition (not persisted to disk, just
    // session-local `Context` memory) so `show_category_nav` can grab keyboard
    // focus for the current category exactly once per open, rather than every
    // frame (which would fight any focus the user later gives a control in the
    // content pane) — see that function's doc comment for why a click used to
    // be required before Up/Down did anything.
    let was_open_id = egui::Id::new("settings_was_open_last_frame");
    let was_open = ctx.data(|d| d.get_temp::<bool>(was_open_id).unwrap_or(false));
    let just_opened = *open && !was_open;
    ctx.data_mut(|d| d.insert_temp(was_open_id, *open));

    if !*open {
        return false;
    }
    let mut changed = false;
    let mut close_requested = false;
    let modal_id = egui::Id::new("settings_modal");
    let modal_response = egui::Modal::new(modal_id)
        .area(
            egui::Modal::default_area(modal_id)
                .default_width(720.0)
                .default_height(520.0),
        )
        .show(ctx, |ui| {
            ui.set_min_size(egui::vec2(720.0, 520.0));
            ui.heading("Settings");
            ui.add_space(4.0);
            egui::Panel::bottom("settings_bottom_bar")
                .resizable(false)
                // Fixed, not auto-sized to content: an auto-sized bottom panel
                // re-measures its content's rect every frame and persists that as
                // its height for the next one (`PanelState`, `containers/panel.rs`)
                // — small per-frame changes in the OK button's own hover-highlight
                // rect fed back into a taller panel next frame, which then measured
                // taller still, compounding into the button visibly climbing while
                // the mouse moved over the dialog. A fixed height sidesteps that
                // feedback loop entirely rather than chasing it.
                .exact_size(40.0)
                .show(ui, |ui| {
                    ui.add_space(4.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("OK").clicked() {
                            close_requested = true;
                        }
                    });
                    ui.add_space(4.0);
                });
            egui::Panel::left("settings_nav")
                .resizable(false)
                .exact_size(160.0)
                .show(ui, |ui| show_category_nav(ui, category, just_opened));
            egui::ScrollArea::vertical().show(ui, |ui| {
                changed |= match *category {
                    SettingsCategory::General => show_general_category(ui, settings),
                    SettingsCategory::Appearance => show_appearance_category(ctx, ui, settings),
                    SettingsCategory::Editor => show_editor_category(ui, settings),
                    SettingsCategory::SpellCheck => show_spell_check_category(
                        ui,
                        settings,
                        dictionary_downloading,
                        dictionary_download_request,
                    ),
                    SettingsCategory::Templates => show_templates_category(ui, settings),
                    SettingsCategory::History => show_history_category(ui, settings),
                    SettingsCategory::Pomodoro => show_pomodoro_category(ui, settings),
                    SettingsCategory::Shortcuts => show_shortcuts_category(
                        ctx,
                        ui,
                        settings,
                        recording_shortcut,
                        plugin_shortcut_rows,
                    ),
                };
            });
        });

    // A click on the dimmed backdrop (outside the dialog) closes it too, standard
    // modal-dialog behavior — separate from `close_requested` (the OK button)
    // only in name; both just mean "the user is done with this dialog."
    if close_requested || modal_response.backdrop_response.clicked() {
        *open = false;
    }

    // Gated on no recording in progress: `show_recording_modal` below also reads
    // Escape (to cancel just the recording, not the whole window) — checking here
    // unconditionally would close Settings out from under it on the same keypress.
    // Deliberately not `ModalResponse::should_close()`, which also reacts to
    // Escape but has no notion of "recording in progress" to gate on.
    if *open && recording_shortcut.is_none() && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        *open = false;
    }

    if let Some(target) = recording_shortcut.clone() {
        changed |= show_recording_modal(
            ctx,
            settings,
            recording_shortcut,
            target,
            plugin_shortcut_rows,
        );
    }

    changed
}

fn show_general_category(ui: &mut egui::Ui, settings: &mut Settings) -> bool {
    let mut changed = false;
    ui.heading("General");
    ui.add_space(12.0);
    changed |= ui
        .checkbox(
            &mut settings.reopen_last_project,
            "Reopen project on launch",
        )
        .changed();
    changed |= ui
        .checkbox(
            &mut settings.create_starter_folders,
            "Ensure Research and Trash folders exist in every project",
        )
        .changed();
    ui.add_space(12.0);
    ui.heading("Notifications");
    ui.add_space(12.0);
    let mut seconds_row = |ui: &mut egui::Ui, label: &str, value: &mut u32, default: u32| {
        ui.horizontal(|ui| {
            ui.label(label);
            let mut seconds = if *value > 0 { *value } else { default };
            if ui
                .add(
                    egui::DragValue::new(&mut seconds)
                        .range(1..=60)
                        .suffix(" sec"),
                )
                .changed()
            {
                *value = seconds;
                changed = true;
            }
        });
    };
    // Defaults here must match `app::DEFAULT_TOAST_DURATION`/
    // `DEFAULT_STATUS_MESSAGE_DURATION` — shown as the starting
    // value for an unconfigured (`0`) setting, same
    // blank-means-unset convention as the Pomodoro durations
    // below.
    seconds_row(
        ui,
        "Error toast duration:",
        &mut settings.toast_duration_secs,
        6,
    );
    seconds_row(
        ui,
        "Status bar message duration:",
        &mut settings.status_message_duration_secs,
        8,
    );
    changed
}

fn show_appearance_category(
    ctx: &egui::Context,
    ui: &mut egui::Ui,
    settings: &mut Settings,
) -> bool {
    let mut changed = false;
    ui.heading("Theme");
    ui.add_space(12.0);
    let previous_theme = settings.theme_preference;
    settings.theme_preference.radio_buttons(ui);
    if settings.theme_preference != previous_theme {
        ctx.set_theme(settings.theme_preference);
        changed = true;
    }
    ui.add_space(12.0);
    ui.heading("Font");
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.label("UI font:");
        let previous_font = settings.ui_font;
        egui::ComboBox::new("ui_font_combo", "")
            .selected_text(settings.ui_font.label())
            .show_ui(ui, |ui| {
                for font in EditorFont::ALL {
                    ui.selectable_value(&mut settings.ui_font, font, font.label());
                }
            });
        changed |= settings.ui_font != previous_font;
    })
    .response
    .on_hover_text(
        "The typeface for menus, the Binder, buttons, and every other UI \
         chrome element — separate from the Editor and Preview font under \
         the Editor category.",
    );
    ui.add_space(12.0);
    ui.heading("UI Scale");
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.label("Scale:");
        let mut percent = (settings.resolve_ui_scale() * 100.0).round() as u32;
        if ui
            .add(
                egui::DragValue::new(&mut percent)
                    .range(50..=300)
                    .suffix("%"),
            )
            .changed()
        {
            settings.ui_scale = percent as f32 / 100.0;
            ctx.set_zoom_factor(settings.ui_scale);
            changed = true;
        }
        if ui.button("Reset").clicked() {
            settings.ui_scale = 0.0;
            ctx.set_zoom_factor(settings.resolve_ui_scale());
            changed = true;
        }
    })
    .response
    .on_hover_text(
        "A manual multiplier on top of whatever this platform's own \
         display scaling already reports — mainly useful when that \
         comes back wrong (some Wayland compositors don't report a \
         scale winit picks up). Leave at 100% if the UI already looks \
         right.",
    );
    ui.add_space(12.0);
    ui.heading("Binder");
    ui.add_space(12.0);
    changed |= ui
        .checkbox(
            &mut settings.show_document_stats_in_binder,
            "Show document stats in binder",
        )
        .on_hover_text(
            "Show each document's line, word, and character count on its \
             Binder row.",
        )
        .changed();
    changed
}

fn show_editor_category(ui: &mut egui::Ui, settings: &mut Settings) -> bool {
    let mut changed = false;
    ui.heading("Font");
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.label("Editor and Preview font:");
        let previous_font = settings.editor_font;
        egui::ComboBox::new("editor_font_combo", "")
            .selected_text(settings.editor_font.label())
            .show_ui(ui, |ui| {
                for font in EditorFont::ALL {
                    ui.selectable_value(&mut settings.editor_font, font, font.label());
                }
            });
        changed |= settings.editor_font != previous_font;
    });
    ui.horizontal(|ui| {
        ui.label("Size:");
        let mut size = crate::editor_font::resolve_size(settings.editor_font_size);
        if ui
            .add(
                egui::DragValue::new(&mut size)
                    .range(8.0..=48.0)
                    .suffix("pt"),
            )
            .changed()
        {
            settings.editor_font_size = size;
            changed = true;
        }
    });
    ui.add_space(12.0);
    ui.heading("Typography");
    ui.add_space(12.0);
    changed |= ui
        .checkbox(
            &mut settings.typewriter_quotes,
            "Typewriter quotes in Preview and export",
        )
        .on_hover_text(
            "Render \" ' -- ... as curly quotes, an em dash, and an \
             ellipsis. Only affects how markdown is rendered here and \
             in exported files — the source .md text you type is never \
             changed.",
        )
        .changed();
    ui.add_space(12.0);
    ui.heading("Gutter");
    ui.add_space(12.0);
    changed |= ui
        .checkbox(&mut settings.show_editor_gutter, "Show line numbers")
        .on_hover_text(
            "A gutter down the left edge of the Editor showing each line's \
             number. Counts logical lines, not wrapped rows — a long \
             paragraph's wrapped continuation isn't numbered again.",
        )
        .changed();
    changed
}

fn show_spell_check_category(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    dictionary_downloading: Option<SpellCheckLanguage>,
    dictionary_download_request: &mut Option<SpellCheckLanguage>,
) -> bool {
    let mut changed = false;
    ui.heading("Spell Check");
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.label("Language:");
        let previous = settings.spell_check_language;
        egui::ComboBox::new("spell_check_language_combo", "")
            .selected_text(settings.spell_check_language.label())
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut settings.spell_check_language,
                    SpellCheckLanguage::Off,
                    SpellCheckLanguage::Off.label(),
                );
                let mut languages: Vec<SpellCheckLanguage> = SpellCheckLanguage::ALL
                    .into_iter()
                    .filter(|lang| *lang != SpellCheckLanguage::Off)
                    .collect();
                languages.sort_by_key(|lang| lang.label());
                for lang in languages {
                    ui.selectable_value(&mut settings.spell_check_language, lang, lang.label());
                }
            });
        changed |= settings.spell_check_language != previous;
    })
    .response
    .on_hover_text(
        "Underlines words not found in the selected dictionary while you type. \
         No right-click suggestions or \"add to dictionary\" yet — expect false \
         positives on names and invented words until a later update.",
    );
    ui.add_space(12.0);
    ui.heading("Dictionaries");
    ui.add_space(4.0);
    ui.weak(
        "Smaragd ships with tiny placeholder word lists only — download a real \
         dictionary here to make spell-check actually useful.",
    );
    ui.add_space(8.0);
    egui::ScrollArea::vertical()
        .id_salt("dictionary_catalog_scroll")
        .max_height(280.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            show_dictionary_catalog(ui, dictionary_downloading, dictionary_download_request);
        });
    changed
}

/// The "Dictionaries" list inside the Spell Check category: one row per
/// `spellcheck::catalog()` entry that isn't `is_blocked()` — a blocked entry
/// (this repo's own review found a license incompatibility) is never even
/// shown here, let alone downloadable, regardless of what its `redistributable`
/// field claims. Rows are sorted alphabetically by language name (not
/// `catalog()`'s own order, which just reflects the order each was reviewed
/// and added), and rendered inside `show_spell_check_category`'s own bounded,
/// scrollable area rather than however tall the full list happens to be — with
/// this many languages, an unbounded list would push the language picker
/// above it off screen. Each row shows the language, its SPDX license
/// identifier inline (full review status/copyright/notes as a hover
/// tooltip), and either a "Download" button, a spinner while
/// `dictionary_downloading` names this row's language, or "Downloaded" once
/// `spellcheck::is_downloaded` confirms the files are already on disk.
/// Clicking "Download" sets `*dictionary_download_request` rather than
/// spawning anything itself — this module has no background-thread machinery
/// of its own, that's `app::dictionary_download`'s job once the caller sees
/// the request.
fn show_dictionary_catalog(
    ui: &mut egui::Ui,
    dictionary_downloading: Option<SpellCheckLanguage>,
    dictionary_download_request: &mut Option<SpellCheckLanguage>,
) {
    let mut entries: Vec<_> = crate::spellcheck::catalog().iter().collect();
    entries.sort_by(|a, b| a.language.cmp(&b.language));
    for entry in entries {
        if entry.is_blocked() {
            continue;
        }
        let Some(language) = SpellCheckLanguage::from_code(&entry.language_code) else {
            continue;
        };
        let is_downloading = dictionary_downloading == Some(language);
        let downloaded = crate::spellcheck::is_downloaded(language);
        let row = ui.horizontal(|ui| {
            ui.label(&entry.language);
            ui.weak(format!("({})", entry.license_spdx));
            if is_downloading {
                ui.add(egui::Spinner::new().size(14.0));
                ui.weak("Downloading…");
            } else if downloaded {
                ui.weak("✓ Downloaded");
            } else {
                let enabled = dictionary_downloading.is_none();
                if ui
                    .add_enabled(enabled, egui::Button::new("Download"))
                    .clicked()
                {
                    *dictionary_download_request = Some(language);
                }
            }
        });
        let notes = if entry.review_status_notes.is_empty() {
            String::new()
        } else {
            format!("\n\n{}", entry.review_status_notes)
        };
        row.response.on_hover_text(format!(
            "License: {}\nReview status: {}\nCopyright: {}{notes}",
            entry.license_spdx, entry.review_status, entry.copyright
        ));
    }
}

fn show_templates_category(ui: &mut egui::Ui, settings: &mut Settings) -> bool {
    let mut changed = false;
    ui.heading("Templates");
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.label("Date format for ${{date}}:");
        changed |= ui
            .text_edit_singleline(&mut settings.template_date_format)
            .on_hover_text("A chrono strftime pattern, e.g. %Y-%m-%d. Blank uses %Y-%m-%d.")
            .changed();
    });
    ui.weak(format!(
        "Preview: {}",
        crate::templates::format_date(&settings.template_date_format)
    ));
    changed
}

fn show_history_category(ui: &mut egui::Ui, settings: &mut Settings) -> bool {
    let mut changed = false;
    ui.heading("Git");
    ui.add_space(12.0);
    let mut enabled = settings.git_integration_enabled();
    if ui
        .checkbox(&mut enabled, "Enable Git integration")
        .on_hover_text(
            "Turns off every git-related feature app-wide: the Versions \
             menu is hidden entirely, the Commit/Push shortcuts and every \
             :git command do nothing, and the \"enable git support?\" \
             prompt on opening a project never appears. Independent of, \
             and stronger than, any individual project's own git setting.",
        )
        .changed()
    {
        settings.git_integration_disabled = !enabled;
        changed = true;
    }

    ui.add_space(20.0);
    ui.heading("Backups");
    ui.add_space(12.0);
    changed |= ui
        .checkbox(&mut settings.backup_enabled, "Enable automatic backups")
        .on_hover_text(
            "Zip the whole project folder into a timestamped snapshot, \
             Scrivener-style, at the points checked below. Off by default.",
        )
        .changed();
    ui.add_enabled_ui(settings.backup_enabled, |ui| {
        changed |= ui
            .checkbox(
                &mut settings.backup_on_open,
                "Back up when opening a project",
            )
            .changed();
        changed |= ui
            .checkbox(
                &mut settings.backup_on_close,
                "Back up when closing a project",
            )
            .changed();
        changed |= ui
            .checkbox(
                &mut settings.backup_on_manual_save,
                "Back up on every manual save (Ctrl+S)",
            )
            .on_hover_text(
                "Not the silent autosave on losing focus or switching \
                 documents — only an explicit save.",
            )
            .changed();
        ui.horizontal(|ui| {
            ui.label("Backups to keep:");
            let mut keep = settings.resolve_backup_keep_count();
            if ui
                .add(egui::DragValue::new(&mut keep).range(1..=100))
                .changed()
            {
                settings.backup_keep_count = keep;
                changed = true;
            }
        });
        ui.horizontal(|ui| {
            ui.label("Backup folder:");
            let mut dir_text = settings
                .resolve_backup_dir()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "(no location available)".to_string());
            ui.add_enabled(
                false,
                egui::TextEdit::singleline(&mut dir_text).desired_width(300.0),
            );
            if ui.button("Browse…").clicked()
                && let Some(picked) = rfd::FileDialog::new().pick_folder()
            {
                settings.backup_dir = Some(picked);
                changed = true;
            }
            if settings.backup_dir.is_some() && ui.button("Reset").clicked() {
                settings.backup_dir = None;
                changed = true;
            }
        });
    });

    changed
}

fn show_pomodoro_category(ui: &mut egui::Ui, settings: &mut Settings) -> bool {
    let mut changed = false;
    let mut duration_row = |ui: &mut egui::Ui, label: &str, value: &mut u32, default: u32| {
        ui.horizontal(|ui| {
            ui.label(label);
            let mut minutes = if *value > 0 { *value } else { default };
            if ui
                .add(
                    egui::DragValue::new(&mut minutes)
                        .range(1..=180)
                        .suffix(" min"),
                )
                .changed()
            {
                *value = minutes;
                changed = true;
            }
        });
    };
    ui.heading("Pomodoro");
    ui.add_space(12.0);
    duration_row(ui, "Work session:", &mut settings.pomodoro_work_minutes, 25);
    duration_row(
        ui,
        "Short break:",
        &mut settings.pomodoro_short_break_minutes,
        5,
    );
    duration_row(
        ui,
        "Long break:",
        &mut settings.pomodoro_long_break_minutes,
        15,
    );
    ui.horizontal(|ui| {
        ui.label("Work sessions before a long break:");
        let mut cycles = if settings.pomodoro_cycles_before_long_break > 0 {
            settings.pomodoro_cycles_before_long_break
        } else {
            4
        };
        if ui
            .add(egui::DragValue::new(&mut cycles).range(1..=12))
            .changed()
        {
            settings.pomodoro_cycles_before_long_break = cycles;
            changed = true;
        }
    });
    ui.add_space(12.0);
    changed |= ui
        .checkbox(
            &mut settings.pomodoro_notifications_enabled,
            "Show a desktop notification when a phase completes",
        )
        .changed();
    changed
}

fn show_shortcuts_category(
    ctx: &egui::Context,
    ui: &mut egui::Ui,
    settings: &mut Settings,
    recording_shortcut: &mut Option<ShortcutTarget>,
    plugin_shortcut_rows: &[(String, Option<egui::KeyboardShortcut>)],
) -> bool {
    let mut changed = false;
    ui.heading("Keyboard Shortcuts");
    ui.add_space(12.0);
    // Sorted by functional category (`ShortcutCategory::ALL`'s order),
    // then alphabetically by label within each — shown as a "Category"
    // column on every row rather than a heading per group, so the whole
    // list stays one scannable grid instead of several disjoint ones.
    let mut actions: Vec<ShortcutAction> = ShortcutAction::ALL.to_vec();
    if !settings.git_integration_enabled() {
        actions.retain(|action| action.category() != ShortcutCategory::Git);
    }
    actions.sort_by_key(|action| {
        let category_index = ShortcutCategory::ALL
            .iter()
            .position(|category| *category == action.category())
            .unwrap_or(usize::MAX);
        (category_index, action.label())
    });

    egui::Grid::new("shortcuts_grid")
        .num_columns(4)
        .striped(true)
        .show(ui, |ui| {
            ui.strong("Category");
            ui.strong("Action");
            ui.strong("Shortcut");
            ui.end_row();

            for action in &actions {
                ui.label(action.category().label());
                ui.label(action.label());
                let text = settings
                    .shortcuts
                    .get(*action)
                    .map(|s| ctx.format_shortcut(&s))
                    .unwrap_or_else(|| "Unbound".to_string());
                ui.label(text);
                ui.horizontal(|ui| {
                    if ui.button("Change").clicked() {
                        *recording_shortcut = Some(ShortcutTarget::BuiltIn(*action));
                    }
                    if ui.button("Clear").clicked() {
                        settings.shortcuts.set(*action, None);
                        changed = true;
                    }
                });
                ui.end_row();
            }
        });

    if !plugin_shortcut_rows.is_empty() {
        ui.separator();
        ui.heading("Plugin Shortcuts");
        ui.add_space(12.0);
        egui::Grid::new("plugin_shortcuts_grid")
            .num_columns(3)
            .striped(true)
            .show(ui, |ui| {
                for (name, current) in plugin_shortcut_rows {
                    ui.label(format!(":{name}"));
                    let text = current
                        .map(|s| ctx.format_shortcut(&s))
                        .unwrap_or_else(|| "Unbound".to_string());
                    ui.label(text);
                    ui.horizontal(|ui| {
                        if ui.button("Change").clicked() {
                            *recording_shortcut = Some(ShortcutTarget::Plugin(name.clone()));
                        }
                        if ui.button("Clear").clicked() {
                            settings.set_plugin_shortcut(name, None, plugin_shortcut_rows);
                            changed = true;
                        }
                    });
                    ui.end_row();
                }
            });
    }
    changed
}

/// Renders the category nav list and its Up/Down keyboard navigation. A
/// standalone function (rather than inlined into `show`'s `Panel::left`
/// closure) specifically so tests can drive it directly against a plain `Ui`
/// — the same reason `binder_panel::show` itself isn't wrapped in a
/// `Window`: a floating `Window`'s on-screen position isn't predictable
/// enough in a headless synthetic-input test to click a specific row by
/// coordinates, but a bare `Ui`'s origin is.
///
/// Click a category to give it keyboard focus (`selectable_value`'s
/// underlying widget doesn't grant focus on click by itself — same gotcha
/// `binder_panel.rs` hit), then Up/Down move between categories the same way
/// `binder_panel::show` moves between rows: only acts once one of these rows
/// actually has focus, so it can't steal Up/Down from, say, the Templates
/// page's text field. `just_opened` (true for exactly one frame per dialog
/// open — see `show`) grants the *current* category's row focus automatically,
/// so Up/Down work immediately without requiring that initial click.
fn show_category_nav(ui: &mut egui::Ui, category: &mut SettingsCategory, just_opened: bool) {
    let mut ids = Vec::new();
    for c in SettingsCategory::ALL {
        let response = ui.selectable_value(category, c, c.label());
        if (response.clicked() && !response.has_focus()) || (just_opened && c == *category) {
            response.request_focus();
        }
        // Reapplied every frame the row has focus, not just on the click that
        // granted it: `request_focus` resets the target's filter to
        // unclaimed, and it takes a frame for `has_focus()` to become true
        // after the request — see `CATEGORY_ARROW_KEYS_FILTER`'s doc comment.
        if response.has_focus() {
            ui.ctx().memory_mut(|mem| {
                mem.set_focus_lock_filter(response.id, CATEGORY_ARROW_KEYS_FILTER)
            });
        }
        ids.push(response.id);
    }
    if let Some(focused_id) = ui.ctx().memory(|mem| mem.focused())
        && let Some(current) = ids.iter().position(|id| *id == focused_id)
    {
        let move_down =
            ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown));
        let move_up = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp));
        let next = if move_down {
            Some((current + 1).min(ids.len() - 1))
        } else if move_up {
            Some(current.saturating_sub(1))
        } else {
            None
        };
        // Same guard `binder_panel::show` uses: skip a no-op re-focus at either
        // end, since `Memory::request_focus` unconditionally resets the
        // target's focus-lock filter even when it's already focused, which
        // would otherwise open a one-frame gap for egui's own arrow-key focus
        // navigation to steal in.
        if let Some(next) = next
            && next != current
        {
            *category = SettingsCategory::ALL[next];
            ui.ctx().memory_mut(|mem| mem.request_focus(ids[next]));
        }
    }
}

/// Modal capturing the next keypress as `action`'s new shortcut. Runs as an
/// `egui::Modal` (matching `name_prompt.rs`'s pattern) to block clicks elsewhere
/// while recording. The captured key event is also explicitly removed from the
/// input queue (rather than just peeked at) so it can't *also* reach whatever
/// widget happens to have focus in the background this same frame — e.g. typing
/// into an open document while trying to record a shortcut.
fn show_recording_modal(
    ctx: &egui::Context,
    settings: &mut Settings,
    recording_shortcut: &mut Option<ShortcutTarget>,
    target: ShortcutTarget,
    plugin_shortcut_rows: &[(String, Option<egui::KeyboardShortcut>)],
) -> bool {
    let mut changed = false;
    let mut cancelled = false;
    let mut captured = None;
    let mut rejected = false;

    ctx.input_mut(|input| {
        let mut handled = false;
        input.events.retain(|event| {
            if handled {
                return true;
            }
            let egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = event
            else {
                return true;
            };
            // A bare modifier press (e.g. just Shift, held down before the real key
            // arrives) shows up as its own `Event::Key` with `key` set to one of
            // egui's physical `ShiftLeft`/`ControlRight`/etc. variants — never a
            // valid shortcut on its own. Drop it without setting `handled`, so
            // scanning continues for the actual key the user is holding it to
            // combine with, rather than capturing e.g. "Ctrl+ShiftLeft" as the
            // whole shortcut the instant Shift goes down.
            if is_bare_modifier_key(*key) {
                return false;
            }
            handled = true;
            if *key == egui::Key::Escape && modifiers.is_none() {
                cancelled = true;
            } else {
                captured = Some(egui::KeyboardShortcut::new(*modifiers, *key));
            }
            false
        });
    });

    if cancelled {
        *recording_shortcut = None;
    } else if let Some(shortcut) = captured {
        if is_safe_binding(&shortcut) {
            match &target {
                ShortcutTarget::BuiltIn(action) => settings.shortcuts.set(*action, Some(shortcut)),
                ShortcutTarget::Plugin(name) => {
                    settings.set_plugin_shortcut(name, Some(shortcut), plugin_shortcut_rows)
                }
            }
            changed = true;
            *recording_shortcut = None;
        } else {
            rejected = true;
        }
    }

    let label = match &target {
        ShortcutTarget::BuiltIn(action) => action.label().to_string(),
        ShortcutTarget::Plugin(name) => format!(":{name}"),
    };
    egui::Modal::new(egui::Id::new("shortcut_recording_modal")).show(ctx, |ui| {
        ui.set_min_width(280.0);
        ui.heading(format!("Press a new shortcut for \"{label}\""));
        ui.label("Press Escape to cancel.");
        if rejected {
            ui.colored_label(
                egui::Color32::from_rgb(200, 60, 60),
                "Shortcuts need Ctrl, Alt, or Shift (function keys and Escape are exempt).",
            );
        }
    });

    changed
}

/// True for the physical "which side" modifier-key variants (`ShiftLeft`,
/// `ControlRight`, etc.) `egui::Key` reports pressing a modifier on its own as —
/// never a valid shortcut by itself, so the recording modal above must skip past
/// these rather than capturing one as the whole shortcut the moment a user starts
/// holding Ctrl or Shift, before the actual key.
fn is_bare_modifier_key(key: egui::Key) -> bool {
    matches!(
        key,
        egui::Key::ShiftLeft
            | egui::Key::ShiftRight
            | egui::Key::ControlLeft
            | egui::Key::ControlRight
            | egui::Key::AltLeft
            | egui::Key::AltRight
            | egui::Key::SuperLeft
            | egui::Key::SuperRight
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn escape_event() -> egui::Event {
        egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    fn frame(
        ctx: &egui::Context,
        open: &mut bool,
        settings: &mut Settings,
        recording_shortcut: &mut Option<ShortcutTarget>,
        events: Vec<egui::Event>,
    ) {
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let mut category = SettingsCategory::General;
        let mut dictionary_download_request = None;
        crate::egui_test_support::run_ui_and_discard(ctx, input, |ui| {
            show(
                ui.ctx(),
                open,
                settings,
                &mut category,
                recording_shortcut,
                &[],
                None,
                &mut dictionary_download_request,
            );
        });
    }

    #[test]
    fn escape_closes_the_window_when_nothing_is_being_recorded() {
        let ctx = egui::Context::default();
        let mut settings = Settings::default();
        let mut open = true;
        let mut recording_shortcut = None;

        frame(
            &ctx,
            &mut open,
            &mut settings,
            &mut recording_shortcut,
            vec![],
        );
        assert!(open, "window should still be open before Escape");

        frame(
            &ctx,
            &mut open,
            &mut settings,
            &mut recording_shortcut,
            vec![escape_event()],
        );
        assert!(!open, "Escape should close the Settings window");
    }

    #[test]
    fn escape_cancels_recording_instead_of_closing_the_window() {
        let ctx = egui::Context::default();
        let mut settings = Settings::default();
        let mut open = true;
        let mut recording_shortcut = Some(ShortcutTarget::BuiltIn(ShortcutAction::Save));

        frame(
            &ctx,
            &mut open,
            &mut settings,
            &mut recording_shortcut,
            vec![escape_event()],
        );
        assert!(
            open,
            "Escape while recording a shortcut should cancel the recording, not close Settings"
        );
        assert!(recording_shortcut.is_none());
    }

    /// Drives the real `show` (full window: bottom bar, nav, and content pane
    /// together) with synthetic input across frames — worth the extra machinery
    /// (mirroring `binder_panel.rs`'s own `Harness`) specifically because this
    /// exact class of bug (keyboard focus racing ahead of the `*category` it's
    /// supposed to be driving) already shipped once undetected by build/clippy/
    /// fmt and a crash-only manual smoke test — and, worse, wasn't caught by an
    /// earlier version of this same test that drove `show_category_nav` in
    /// isolation: the divergence only reproduces with the *other* widgets in the
    /// window present too (egui's own built-in arrow-key focus traversal needs
    /// something else to jump focus to), so the harness has to go through the
    /// real `Window`, not a bare `Ui`.
    struct FullHarness {
        ctx: egui::Context,
        open: bool,
        settings: Settings,
        category: SettingsCategory,
        recording_shortcut: Option<ShortcutTarget>,
    }

    impl Default for FullHarness {
        fn default() -> Self {
            FullHarness {
                ctx: egui::Context::default(),
                open: true,
                settings: Settings::default(),
                category: SettingsCategory::General,
                recording_shortcut: None,
            }
        }
    }

    impl FullHarness {
        fn frame(&mut self, events: Vec<egui::Event>) {
            let input = egui::RawInput {
                // Modal centers itself on the context's `content_rect` — an
                // explicit, generously-sized screen rect makes that centered
                // position deterministic for `click_first_row`'s scan below,
                // rather than whatever egui's own zero-size default falls back to.
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 900.0),
                )),
                events,
                ..Default::default()
            };
            let (open, settings, category, recording_shortcut) = (
                &mut self.open,
                &mut self.settings,
                &mut self.category,
                &mut self.recording_shortcut,
            );
            let mut dictionary_download_request = None;
            crate::egui_test_support::run_ui_and_discard(&self.ctx, input, |ui| {
                show(
                    ui.ctx(),
                    open,
                    settings,
                    category,
                    recording_shortcut,
                    &[],
                    None,
                    &mut dictionary_download_request,
                );
            });
        }

        fn settle(&mut self) {
            self.ctx.all_styles_mut(|style| style.animation_time = 0.0);
            self.frame(vec![]);
            self.frame(vec![]);
        }

        fn click(&mut self, pos: egui::Pos2) {
            self.frame(vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }

        /// One frame with no input — see `binder_panel.rs`'s `Harness::press` doc
        /// comment for why a real gap frame is needed between a focus change and
        /// the next keypress: egui's own focus-lock filter only takes effect
        /// starting the *second* frame a widget has focus.
        fn idle(&mut self) {
            self.frame(vec![]);
        }

        fn press(&mut self, key: egui::Key) {
            self.frame(vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }]);
        }

        fn focused(&self) -> Option<egui::Id> {
            self.ctx.memory(|mem| mem.focused())
        }

        /// Click across a grid covering the whole default window size until
        /// keyboard focus actually changes — reaches the first nav row
        /// ("General") without needing to know the floating `Window`'s
        /// on-screen position (unlike `binder_panel.rs`'s un-windowed root
        /// panel, `Settings` is a `Window`, whose default placement isn't
        /// something a test can just assume).
        fn click_first_row(&mut self) -> egui::Id {
            let before = self.focused();
            // Strictly inside the modal's content area (measured: a centered
            // 720x520 modal on the 1200x900 `screen_rect` set in `frame` lands
            // around (240, 190)..(960, 710), with the nav list starting a bit
            // further in past the heading/frame margin). Deliberately does NOT
            // extend out to the modal's own edges or beyond: a click outside the
            // modal is a click on the backdrop, which closes the dialog (working
            // as intended) — do that once by accident here and every subsequent
            // attempt in this scan silently renders nothing, since `show` returns
            // immediately once `*open` is false.
            for y in (215..350).step_by(5) {
                for x in (245..500).step_by(5) {
                    let pos = egui::pos2(x as f32, y as f32);
                    self.click(pos);
                    if let Some(after) = self.focused()
                        && Some(after) != before
                    {
                        return after;
                    }
                }
            }
            panic!("clicking across the modal's nav list never changed keyboard focus");
        }
    }

    #[test]
    fn opening_the_dialog_focuses_the_general_category_without_a_click() {
        let mut harness = FullHarness::default();
        harness.settle();

        assert!(
            harness.focused().is_some(),
            "General's row should already have keyboard focus as soon as the \
             dialog opens, with no click needed"
        );
        assert_eq!(harness.category, SettingsCategory::General);
    }

    #[test]
    fn clicking_a_different_category_moves_keyboard_focus_there() {
        let mut harness = FullHarness::default();
        harness.settle();
        let initial_focus = harness.focused().unwrap(); // General, auto-focused on open

        let after_click = harness.click_first_row();
        assert_ne!(initial_focus, after_click);
        assert!(harness.focused().is_some());
    }

    #[test]
    fn arrow_down_moves_both_focus_and_the_selected_category_in_lockstep() {
        let mut harness = FullHarness::default();
        harness.settle();
        // General already has focus as soon as the dialog opens — no click needed.
        let first_focus = harness.focused().unwrap();
        assert_eq!(harness.category, SettingsCategory::General);
        harness.idle();

        harness.press(egui::Key::ArrowDown);

        // The regression this guards: without a claimed focus-lock filter,
        // egui's own built-in arrow-key focus traversal also reacts to
        // ArrowDown, moving keyboard focus to the next row on its own —
        // independently of, and racing with, the manual handling that updates
        // `category`. That leaves focus one row ahead of the category the
        // content pane actually shows, rather than the two changing together.
        let second_focus = harness.focused().unwrap();
        assert_ne!(
            first_focus, second_focus,
            "ArrowDown should move focus to a different row"
        );
        assert_eq!(
            harness.category,
            SettingsCategory::Appearance,
            "ArrowDown should also advance the selected category, in the same frame focus moves"
        );
    }

    #[test]
    fn arrow_down_does_not_move_past_the_last_category() {
        let mut harness = FullHarness::default();
        harness.settle();

        for _ in 0..(SettingsCategory::ALL.len() + 2) {
            harness.idle();
            harness.press(egui::Key::ArrowDown);
        }

        assert_eq!(harness.category, SettingsCategory::Shortcuts);
    }
}
