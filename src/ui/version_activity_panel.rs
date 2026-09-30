//! The Version Activity dock tab: current dirty files, recent commit history,
//! and a log of what smaragd itself has done (or tried to do) via git this
//! session — auto-commits included. Pure rendering, like `sync_panel`: the
//! caller derives a [`VersionActivityData`] from its state each frame and
//! handles the returned [`VersionActivityEvent`].

use std::collections::VecDeque;

use crate::git::{CommitLogEntry, GitActivityEntry, GitActivityOutcome};

/// Everything the panel shows, borrowed from the app's state.
pub struct VersionActivityData<'a> {
    pub has_project: bool,
    /// `Settings::git_integration_enabled() && ProjectMeta::git_enabled`.
    pub git_available: bool,
    /// Dirty file paths, already relative to the project root for display,
    /// sorted.
    pub dirty_files: &'a [String],
    pub commits: &'a [CommitLogEntry],
    pub activity: &'a VecDeque<GitActivityEntry>,
    /// Seconds since the Unix epoch, to phrase `activity` entries' ages via
    /// `ui::sync_panel::humanize_age`.
    pub now_unix: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionActivityEvent {
    Refresh,
}

pub fn show(ui: &mut egui::Ui, data: &VersionActivityData) -> Option<VersionActivityEvent> {
    let mut event = None;
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.add_space(8.0);
        if !data.has_project {
            ui.label("Open a project folder to get started.");
            return;
        }
        if !data.git_available {
            ui.label("Git integration isn't turned on for this project.");
            ui.weak(
                "Enable it from the Versions menu, or Project Settings if the \
                 global switch in Settings > History is already on.",
            );
            return;
        }
        if ui.button("Refresh").clicked() {
            event = Some(VersionActivityEvent::Refresh);
        }
        ui.add_space(10.0);

        ui.strong(format!("Dirty files ({})", data.dirty_files.len()));
        ui.add_space(4.0);
        if data.dirty_files.is_empty() {
            ui.weak("Nothing to commit.");
        } else {
            for path in data.dirty_files {
                ui.label(path);
            }
        }

        ui.add_space(14.0);
        ui.separator();
        ui.add_space(10.0);
        ui.strong("Recent commits");
        ui.add_space(4.0);
        if data.commits.is_empty() {
            ui.weak("No commits yet.");
        } else {
            for commit in data.commits {
                ui.horizontal(|ui| {
                    ui.weak(short_hash(&commit.hash));
                    ui.label(&commit.subject);
                });
                ui.weak(format!("{} — {}", commit.author, commit.relative_date));
                ui.add_space(4.0);
            }
        }

        ui.add_space(14.0);
        ui.separator();
        ui.add_space(10.0);
        ui.strong("Activity");
        ui.add_space(4.0);
        if data.activity.is_empty() {
            ui.weak("Nothing yet this session.");
        } else {
            for entry in data.activity {
                let color = match entry.outcome {
                    GitActivityOutcome::Success => None,
                    GitActivityOutcome::Neutral => Some(ui.visuals().weak_text_color()),
                    GitActivityOutcome::Error => Some(ui.visuals().error_fg_color),
                };
                ui.horizontal(|ui| {
                    let age = crate::ui::sync_panel::humanize_age(
                        data.now_unix.saturating_sub(entry.at_unix),
                    );
                    ui.weak(age);
                    match color {
                        Some(color) => {
                            ui.colored_label(color, &entry.message);
                        }
                        None => {
                            ui.label(&entry.message);
                        }
                    }
                });
            }
        }
    });
    event
}

/// The first 7 characters of a commit hash — the conventional "short hash"
/// length `git log --oneline`/GitHub/etc. all default to.
fn short_hash(hash: &str) -> String {
    hash.chars().take(7).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::egui_test_support::run_ui_and_discard;

    fn sample_data() -> (Vec<String>, Vec<CommitLogEntry>, VecDeque<GitActivityEntry>) {
        let dirty = vec!["Chapter One.md".to_string()];
        let commits = vec![CommitLogEntry {
            hash: "abcdef1234567890".to_string(),
            author: "Author".to_string(),
            relative_date: "2 hours ago".to_string(),
            subject: "Smaragd backup".to_string(),
        }];
        let mut activity = VecDeque::new();
        activity.push_front(GitActivityEntry {
            at_unix: 1_000_000,
            message: "Committed".to_string(),
            outcome: GitActivityOutcome::Success,
        });
        activity.push_front(GitActivityEntry {
            at_unix: 1_000_050,
            message: "Push failed: network unreachable".to_string(),
            outcome: GitActivityOutcome::Error,
        });
        (dirty, commits, activity)
    }

    #[test]
    fn renders_without_panicking_in_every_state() {
        let ctx = egui::Context::default();
        let (dirty, commits, activity) = sample_data();

        for data in [
            VersionActivityData {
                has_project: false,
                git_available: false,
                dirty_files: &[],
                commits: &[],
                activity: &VecDeque::new(),
                now_unix: 1_000_100,
            },
            VersionActivityData {
                has_project: true,
                git_available: false,
                dirty_files: &[],
                commits: &[],
                activity: &VecDeque::new(),
                now_unix: 1_000_100,
            },
            VersionActivityData {
                has_project: true,
                git_available: true,
                dirty_files: &[],
                commits: &[],
                activity: &VecDeque::new(),
                now_unix: 1_000_100,
            },
            VersionActivityData {
                has_project: true,
                git_available: true,
                dirty_files: &dirty,
                commits: &commits,
                activity: &activity,
                now_unix: 1_000_100,
            },
        ] {
            run_ui_and_discard(&ctx, egui::RawInput::default(), |ui| {
                show(ui, &data);
            });
        }
    }

    #[test]
    fn short_hash_takes_the_first_seven_characters() {
        assert_eq!(short_hash("abcdef1234567890"), "abcdef1");
        assert_eq!(short_hash("abc"), "abc");
    }
}
