//! Background sync inside the app: starts and stops the engine's runner as the settings,
//! project and pairing change, turns its events into UI state, and runs the one-off
//! server operations (create/join a vault, pairing tickets, device management) on
//! threads so the UI never waits on the network.
//!
//! Modeled on how collaboration is wired (`collab.rs`): a session object polled once a
//! frame, whose events become status messages, toasts and repaints. `sync_stub.rs` is the
//! browser build's no-op twin.
//!
//! Merging remote edits into a file the user has open is deliberately *not* done through
//! the editor buffer. While a file has unsaved edits the engine is told to hold it
//! ([`SyncState::update_held`]): the merged text isn't written until the user saves, at
//! which point their edit is merged with whatever arrived. A clean open file is simply
//! rewritten on disk and reloaded by the existing external-change scan
//! (`external_watch.rs`), as for any other outside change.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use smaragd_sync_protocol::VaultId;
use smaragd_sync_protocol::api::DeviceInfo;
use smaragd_sync_protocol::ticket::SyncTicket;

use super::SmaragdApp;
use super::dock::DockTab;
use super::prompt::{PendingPrompt, PromptAction};
use crate::settings::Settings;
use crate::sync::engine::SyncReport;
use crate::sync::link::{DeviceCredentials, ProjectLink, data_root, state_dir};
use crate::sync::pairing::{self, PairError, Paired};
use crate::sync::runner::{self, HaltReason, StartParams, SyncEvent, SyncRunner};
use crate::sync::state::DirStateStore;
use crate::ui::name_prompt::NamePromptState;
use crate::ui::settings_panel::{SettingsCategory, SyncTestStatus};
use crate::ui::sync_panel::{SyncPanelData, SyncPanelEvent, SyncPanelPhase, humanize_age};

/// How often a pass runs on its own (saves and the panel's Sync Now trigger one sooner).
const SYNC_INTERVAL: Duration = Duration::from_secs(10);

/// What the runner was started for. A change (another project, a new passphrase, a
/// different server, sync switched off) stops it and, unless it had halted for this exact
/// configuration, starts a fresh one.
#[derive(Clone, PartialEq, Eq)]
struct Signature {
    root: PathBuf,
    vault: VaultId,
    passphrase: String,
    server: String,
}

enum Phase {
    Starting,
    Syncing,
    UpToDate(Instant),
    Offline(String),
    Halted(String),
}

/// The result of a one-off server operation, sent back from its thread.
enum TaskOutcome {
    Tested(Result<String, String>),
    Paired(Result<Paired, PairError>),
    Ticket(Result<SyncTicket, PairError>),
    Devices(Result<Vec<DeviceInfo>, PairError>),
    Revoked(Result<(), PairError>),
    Left(Result<bool, PairError>),
}

pub(super) struct SyncState {
    /// The project root the cached link/credentials below were loaded for.
    link_for: Option<PathBuf>,
    link: Option<ProjectLink>,
    credentials: Option<DeviceCredentials>,
    server_label: Option<String>,
    runner: Option<SyncRunner>,
    running_for: Option<Signature>,
    halted_for: Option<Signature>,
    phase: Phase,
    last_synced_label: String,
    activity: Option<String>,
    held_sent: BTreeSet<String>,
    task: Option<Receiver<TaskOutcome>>,
    notice: Option<String>,
    ticket: Option<String>,
    devices: Option<Vec<DeviceInfo>>,
    /// Documents the last pass couldn't upload because a change is too large, as last
    /// announced — a toast only when this changes, not on every pass.
    too_large: Vec<String>,
}

impl Default for SyncState {
    fn default() -> Self {
        Self {
            link_for: None,
            link: None,
            credentials: None,
            server_label: None,
            runner: None,
            running_for: None,
            halted_for: None,
            phase: Phase::Starting,
            last_synced_label: String::new(),
            activity: None,
            held_sent: BTreeSet::new(),
            task: None,
            notice: None,
            ticket: None,
            devices: None,
            too_large: Vec::new(),
        }
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// A short account of what a pass did, or `None` if it did nothing worth mentioning.
fn summarize(report: &SyncReport) -> Option<String> {
    let mut parts = Vec::new();
    let updated = report.files_written;
    if updated > 0 {
        parts.push(format!("updated {}", plural(updated, "file", "files")));
    }
    if report.files_renamed > 0 {
        parts.push(format!(
            "renamed {}",
            plural(report.files_renamed, "file", "files")
        ));
    }
    if report.files_removed > 0 {
        parts.push(format!(
            "removed {}",
            plural(report.files_removed, "file", "files")
        ));
    }
    if report.meta_written {
        parts.push("updated project settings".to_string());
    }
    let mut text = parts.join(", ");
    if report.pushed_updates > 0 {
        let sent = format!(
            "sent {}",
            plural(report.pushed_updates, "change", "changes")
        );
        text = if text.is_empty() {
            sent
        } else {
            format!("{text}; {sent}")
        };
    }
    if report.files_held > 0 {
        let waiting = format!(
            "{} waiting for you to save",
            plural(report.files_held, "file", "files")
        );
        text = if text.is_empty() {
            waiting
        } else {
            format!("{text}; {waiting}")
        };
    }
    (!text.is_empty()).then(|| {
        let mut chars = text.chars();
        chars
            .next()
            .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
            .unwrap_or_default()
    })
}

fn halt_message(reason: &HaltReason) -> String {
    match reason {
        HaltReason::WrongPassphrase => {
            "The sync passphrase doesn't match this vault. Correct it in Settings > Sync; \
             sync restarts by itself."
                .into()
        }
        HaltReason::Revoked => {
            "This device was removed from the vault, or the vault was deleted. Stop syncing \
             this project, then join again with a new ticket if you still want it synced."
                .into()
        }
        HaltReason::ProjectMissing => "The project folder is missing.".into(),
        HaltReason::Fatal(message) => format!("Sync stopped: {message}"),
    }
}

/// The Sync panel's explanation for documents that can't be uploaded.
fn too_large_message(docs: &[String]) -> String {
    let which = match docs {
        [one] => format!("\u{201c}{one}\u{201d} has"),
        many => format!("{} documents have", many.len()),
    };
    format!(
        "{which} a change too large to upload (over 8 MB at once), so it isn't syncing. \
         Everything else still syncs. Remove the oversized content and save, and it \
         syncs again."
    )
}

/// `path` relative to `root`, `/`-separated — the form the engine's held set uses.
fn relative_key(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let parts: Option<Vec<&str>> = rel.components().map(|c| c.as_os_str().to_str()).collect();
    Some(parts?.join("/"))
}

impl SyncState {
    /// What the Sync dock tab should show. `has_project` and `settings` are passed in
    /// (rather than read from the app) so this can be called while the dock state is
    /// mutably borrowed.
    pub(super) fn panel_data<'a>(
        &'a self,
        settings: &'a Settings,
        has_project: bool,
    ) -> SyncPanelData<'a> {
        let phase = if !settings.sync_enabled {
            SyncPanelPhase::Off
        } else if !has_project {
            SyncPanelPhase::NoProject
        } else if let Some(problem) = settings.sync_config_problem() {
            SyncPanelPhase::NeedsSettings(problem)
        } else if self.link.is_none() || self.credentials.is_none() {
            SyncPanelPhase::NotPaired
        } else {
            match &self.phase {
                Phase::Starting => SyncPanelPhase::Starting,
                Phase::Syncing => SyncPanelPhase::Syncing,
                Phase::UpToDate(_) => SyncPanelPhase::UpToDate {
                    last_synced: &self.last_synced_label,
                },
                Phase::Offline(reason) => SyncPanelPhase::Offline { reason },
                Phase::Halted(message) => SyncPanelPhase::Halted { message },
            }
        };
        SyncPanelData {
            phase,
            busy: self.task.is_some(),
            server: self.server_label.as_deref(),
            activity: self.activity.as_deref(),
            ticket: self.ticket.as_deref(),
            devices: self.devices.as_deref(),
            own_device: self.credentials.as_ref().map(|c| c.device_id),
            now_unix: unix_now(),
            notice: self.notice.as_deref(),
        }
    }

    fn stop_runner(&mut self) {
        self.runner = None;
        self.running_for = None;
    }

    /// Forgets everything tied to the previous project or pairing.
    fn reset_view(&mut self) {
        self.phase = Phase::Starting;
        self.activity = None;
        self.notice = None;
        self.ticket = None;
        self.devices = None;
        self.held_sent.clear();
        self.halted_for = None;
        self.too_large.clear();
    }

    fn refresh_label(&mut self) {
        if let Phase::UpToDate(at) = self.phase {
            self.last_synced_label = humanize_age(at.elapsed().as_secs());
        }
    }

    /// Loads the link and credentials for `project_root` from disk.
    fn load_pairing(
        &mut self,
        store: &dyn crate::project::store::ProjectStore,
        root: &Path,
        data: Option<PathBuf>,
    ) {
        self.link = ProjectLink::load(store, root);
        self.credentials = None;
        if let (Some(link), Some(data)) = (&self.link, data) {
            let files: std::sync::Arc<dyn crate::project::store::ProjectStore> =
                crate::project::store::native_store();
            let state = DirStateStore::new(files, state_dir(&data, link.vault_id));
            self.credentials = DeviceCredentials::load(&state);
        }
        self.server_label = self.link.as_ref().map(pairing::describe);
    }
}

impl SmaragdApp {
    /// Where sync keeps credentials and engine state — `None` for a test fixture, so no
    /// test can ever start a real runner or write into the developer's own data
    /// directory (same idea as the fixture's `settings_path_override`).
    fn sync_data_root(&self) -> Option<PathBuf> {
        if self.is_test_fixture {
            None
        } else {
            data_root()
        }
    }

    /// Called once per frame, near the top of `ui()`.
    pub(super) fn poll_sync(&mut self, ctx: &egui::Context) {
        self.sync_refresh_pairing();
        self.sync_drain_task(ctx);
        self.sync_drain_events();
        self.sync_ensure_running(ctx);
        self.sync_update_held();
        self.sync_handle_settings_test(ctx);
        self.sync.refresh_label();
        if self.sync.runner.is_some() {
            // Keeps "synced 3 min ago" fresh while nothing else is repainting.
            ctx.request_repaint_after(Duration::from_secs(20));
        }
    }

    /// Notices a project being opened, closed or switched, and loads its pairing.
    fn sync_refresh_pairing(&mut self) {
        let root = self.project.as_ref().map(|project| project.root.clone());
        if root == self.sync.link_for {
            return;
        }
        self.sync.stop_runner();
        self.sync.reset_view();
        self.sync.link_for = root.clone();
        self.sync.link = None;
        self.sync.credentials = None;
        self.sync.server_label = None;
        let data = self.sync_data_root();
        if let (Some(project), Some(root)) = (&self.project, &root) {
            self.sync.load_pairing(project.store.as_ref(), root, data);
        }
    }

    fn sync_signature(&self) -> Option<Signature> {
        if !self.settings.sync_enabled || self.settings.sync_config_problem().is_some() {
            return None;
        }
        let (project, link) = (self.project.as_ref()?, self.sync.link.as_ref()?);
        self.sync.credentials.as_ref()?;
        Some(Signature {
            root: project.root.clone(),
            vault: link.vault_id,
            passphrase: self.settings.sync_passphrase.0.clone(),
            server: link.server.base_url(),
        })
    }

    /// Starts, restarts or stops the runner so it matches the settings and pairing.
    fn sync_ensure_running(&mut self, ctx: &egui::Context) {
        let wanted = self.sync_signature();
        if wanted == self.sync.running_for {
            return;
        }
        self.sync.stop_runner();
        let Some(signature) = wanted else {
            return;
        };
        if self.sync.halted_for.as_ref() == Some(&signature) {
            return;
        }
        self.sync.halted_for = None;
        let (Some(project), Some(link), Some(credentials), Some(data)) = (
            self.project.as_ref(),
            self.sync.link.clone(),
            self.sync.credentials.clone(),
            self.sync_data_root(),
        ) else {
            return;
        };
        let params = StartParams {
            state_dir: state_dir(&data, link.vault_id),
            link,
            credentials,
            passphrase: self.settings.sync_passphrase.0.clone(),
            project_root: project.root.clone(),
            files: project.store.clone(),
        };
        let repaint = ctx.clone();
        self.sync.phase = Phase::Starting;
        self.sync.runner = Some(runner::start(
            params,
            SYNC_INTERVAL,
            Box::new(move || repaint.request_repaint()),
        ));
        self.sync.running_for = Some(signature);
    }

    fn sync_drain_events(&mut self) {
        let events = self
            .sync
            .runner
            .as_ref()
            .map(SyncRunner::poll)
            .unwrap_or_default();
        for event in events {
            match event {
                SyncEvent::Syncing => {
                    if !matches!(self.sync.phase, Phase::UpToDate(_)) {
                        self.sync.phase = Phase::Syncing;
                    }
                }
                SyncEvent::Synced(report) => self.sync_handle_report(report),
                SyncEvent::Offline(reason) => self.sync.phase = Phase::Offline(reason),
                SyncEvent::Failed(reason) => {
                    self.sync.phase = Phase::Offline(format!("The last pass failed: {reason}"));
                }
                SyncEvent::Halted(reason) => {
                    let message = halt_message(&reason);
                    self.push_error_toast(message.clone());
                    self.sync.halted_for = self.sync.running_for.take();
                    self.sync.runner = None;
                    self.sync.phase = Phase::Halted(message);
                }
            }
        }
    }

    fn sync_handle_report(&mut self, report: SyncReport) {
        self.sync.phase = Phase::UpToDate(Instant::now());
        self.sync.last_synced_label = humanize_age(0);
        self.sync.notice = None;
        let touched_disk = report.files_written
            + report.files_renamed
            + report.files_removed
            + report.dirs_created
            + report.dirs_removed
            > 0;
        if let Some(project) = self.project.as_mut() {
            let meta_changed = report.meta_written && project.reload_metadata();
            if touched_disk || meta_changed {
                project.rescan();
                self.document_status_cache.clear();
            }
        }
        if let Some(summary) = summarize(&report) {
            self.set_status_message(format!("Sync: {summary}"));
            self.sync.activity = Some(summary);
        }
        if report.too_large.is_empty() {
            self.sync.too_large.clear();
        } else {
            let message = too_large_message(&report.too_large);
            if report.too_large != self.sync.too_large {
                self.push_error_toast(message.clone());
                self.sync.too_large = report.too_large;
            }
            self.sync.notice = Some(message);
        }
        for copy in &report.conflict_copies {
            self.push_error_toast(format!(
                "Sync kept your own version as a separate file: {copy}. Compare the two and merge by hand."
            ));
        }
    }

    /// Tells the engine which file (if any) has unsaved edits, so it isn't overwritten.
    fn sync_update_held(&mut self) {
        let mut held = BTreeSet::new();
        if self.editor.dirty
            && let (Some(project), Some(path)) = (&self.project, &self.editor.open_path)
            && let Some(key) = relative_key(&project.root, path)
        {
            held.insert(key);
        }
        if held != self.sync.held_sent {
            if let Some(runner) = &self.sync.runner {
                runner.set_held_paths(held.clone());
            }
            self.sync.held_sent = held;
        }
    }

    /// Called after a document is saved: release it and sync soon.
    pub(super) fn sync_after_save(&mut self) {
        self.sync_update_held();
        if let Some(runner) = &self.sync.runner {
            runner.sync_now();
        }
    }

    // --- one-off server operations ------------------------------------------------

    fn sync_spawn(
        &mut self,
        ctx: &egui::Context,
        work: impl FnOnce() -> TaskOutcome + Send + 'static,
    ) {
        if self.sync.task.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let repaint = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(work());
            repaint.request_repaint();
        });
        self.sync.task = Some(rx);
        self.sync.notice = None;
    }

    fn sync_drain_task(&mut self, ctx: &egui::Context) {
        let Some(receiver) = &self.sync.task else {
            return;
        };
        let outcome = match receiver.try_recv() {
            Ok(outcome) => outcome,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                self.sync.task = None;
                return;
            }
        };
        self.sync.task = None;
        match outcome {
            TaskOutcome::Tested(result) => {
                self.sync_settings_ui.test_status = match result {
                    Ok(message) => SyncTestStatus::Reachable(message),
                    Err(message) => SyncTestStatus::Failed(message),
                };
            }
            TaskOutcome::Paired(Ok(paired)) => {
                self.sync.reset_view();
                self.sync.server_label = Some(pairing::describe(&paired.link));
                self.sync.link = Some(paired.link);
                self.sync.credentials = Some(paired.credentials);
                self.set_status_message("Sync is set up for this project");
            }
            TaskOutcome::Paired(Err(PairError::NeedsAdminToken)) => {
                self.prompt = Some(PendingPrompt {
                    action: PromptAction::SyncAdminToken,
                    state: NamePromptState::new(
                        "This server needs its admin token to create a vault",
                        "Create Vault",
                        "",
                    ),
                });
            }
            TaskOutcome::Paired(Err(error)) => self.sync.notice = Some(error.to_string()),
            TaskOutcome::Ticket(Ok(ticket)) => self.sync.ticket = Some(ticket.encode()),
            TaskOutcome::Devices(Ok(devices)) => self.sync.devices = Some(devices),
            TaskOutcome::Revoked(Ok(())) => {
                self.set_status_message("Device removed from the vault");
                self.sync_refresh_devices(ctx);
            }
            TaskOutcome::Left(Ok(removed_from_server)) => {
                self.sync.stop_runner();
                self.sync.reset_view();
                self.sync.link = None;
                self.sync.credentials = None;
                self.sync.server_label = None;
                self.set_status_message(if removed_from_server {
                    "Stopped syncing this project"
                } else {
                    "Stopped syncing here. The server couldn't be reached, so ask another device to remove this one from the vault."
                });
            }
            TaskOutcome::Ticket(Err(e))
            | TaskOutcome::Devices(Err(e))
            | TaskOutcome::Revoked(Err(e))
            | TaskOutcome::Left(Err(e)) => self.sync.notice = Some(e.to_string()),
        }
    }

    /// Runs "Test Connection" if the Settings page asked for it.
    fn sync_handle_settings_test(&mut self, ctx: &egui::Context) {
        if !std::mem::take(&mut self.sync_settings_ui.test_requested) {
            return;
        }
        let Some(server) = self.settings.sync_server_addr() else {
            return;
        };
        self.sync_settings_ui.test_status = SyncTestStatus::Testing;
        self.sync_spawn(ctx, move || {
            TaskOutcome::Tested(match pairing::test_connection(&server) {
                Ok(health) => Ok(format!("Connected — server version {}.", health.version)),
                Err(error) => Err(error.to_string()),
            })
        });
    }

    fn sync_refresh_devices(&mut self, ctx: &egui::Context) {
        let (Some(link), Some(credentials)) =
            (self.sync.link.clone(), self.sync.credentials.clone())
        else {
            return;
        };
        self.sync_spawn(ctx, move || {
            TaskOutcome::Devices(pairing::list_devices(&link, &credentials))
        });
    }

    fn sync_create_vault(&mut self, ctx: &egui::Context, admin_token: Option<String>) {
        let (Some(project), Some(server), Some(data)) = (
            self.project.as_ref(),
            self.settings.sync_server_addr(),
            self.sync_data_root(),
        ) else {
            self.sync.notice = Some("Set the sync server in Settings > Sync first.".into());
            return;
        };
        let (files, root) = (project.store.clone(), project.root.clone());
        let device_name = self.settings.resolve_sync_device_name();
        self.sync_spawn(ctx, move || {
            TaskOutcome::Paired(pairing::create_vault(
                &files,
                &root,
                &data,
                &server,
                admin_token.as_deref(),
                &device_name,
            ))
        });
    }

    /// The Join Vault prompt was confirmed with a pasted ticket.
    pub(super) fn sync_join_with_ticket(&mut self, ctx: &egui::Context, pasted: &str) {
        let Ok(ticket) = SyncTicket::decode(pasted) else {
            self.sync.notice = Some("That doesn't look like a pairing ticket.".into());
            return;
        };
        let (Some(project), Some(data)) = (self.project.as_ref(), self.sync_data_root()) else {
            return;
        };
        let (files, root) = (project.store.clone(), project.root.clone());
        let device_name = self.settings.resolve_sync_device_name();
        self.sync_spawn(ctx, move || {
            TaskOutcome::Paired(pairing::join_vault(
                &files,
                &root,
                &data,
                &ticket,
                &device_name,
            ))
        });
    }

    /// The admin-token prompt was confirmed.
    pub(super) fn sync_create_vault_with_token(&mut self, ctx: &egui::Context, token: &str) {
        self.sync_create_vault(ctx, Some(token.to_string()));
    }

    pub(super) fn handle_sync_panel_event(&mut self, ctx: &egui::Context, event: SyncPanelEvent) {
        match event {
            SyncPanelEvent::OpenSettings => {
                self.settings_category = SettingsCategory::Sync;
                self.show_settings = true;
            }
            SyncPanelEvent::CreateVault => self.sync_create_vault(ctx, None),
            SyncPanelEvent::JoinVault => {
                self.prompt = Some(PendingPrompt {
                    action: PromptAction::SyncJoinTicket,
                    state: NamePromptState::new(
                        "Join a vault: paste the pairing ticket from your other device",
                        "Join",
                        "",
                    ),
                });
            }
            SyncPanelEvent::SyncNow => {
                if let Some(runner) = &self.sync.runner {
                    runner.sync_now();
                }
            }
            SyncPanelEvent::Retry => self.sync.halted_for = None,
            SyncPanelEvent::MakeTicket => {
                if let (Some(link), Some(credentials)) =
                    (self.sync.link.clone(), self.sync.credentials.clone())
                {
                    self.sync_spawn(ctx, move || {
                        TaskOutcome::Ticket(pairing::make_ticket(&link, &credentials))
                    });
                }
            }
            SyncPanelEvent::RefreshDevices => self.sync_refresh_devices(ctx),
            SyncPanelEvent::Revoke(device) => {
                if let (Some(link), Some(credentials)) =
                    (self.sync.link.clone(), self.sync.credentials.clone())
                {
                    self.sync_spawn(ctx, move || {
                        TaskOutcome::Revoked(pairing::revoke_device(&link, &credentials, device))
                    });
                }
            }
            SyncPanelEvent::LeaveVault => {
                if let (Some(project), Some(link), Some(credentials), Some(data)) = (
                    self.project.as_ref(),
                    self.sync.link.clone(),
                    self.sync.credentials.clone(),
                    self.sync_data_root(),
                ) {
                    let (files, root) = (project.store.clone(), project.root.clone());
                    self.sync.stop_runner();
                    self.sync_spawn(ctx, move || {
                        TaskOutcome::Left(pairing::leave_vault(
                            &files,
                            &root,
                            &data,
                            &link,
                            &credentials,
                        ))
                    });
                }
            }
        }
    }

    /// Menu: "Sync Now".
    pub(super) fn sync_now(&mut self) {
        if let Some(runner) = &self.sync.runner {
            runner.sync_now();
        }
    }

    /// Menu: whether "Sync Now" makes sense.
    pub(super) fn sync_is_running(&self) -> bool {
        self.sync.runner.is_some()
    }

    /// Menu: open (or focus) the Sync dock tab.
    pub(super) fn show_sync_panel(&mut self) {
        self.toggle_dock_tab(DockTab::Sync);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> SyncReport {
        SyncReport::default()
    }

    #[test]
    fn a_quiet_pass_has_nothing_to_report() {
        assert_eq!(summarize(&report()), None);
    }

    #[test]
    fn incoming_changes_and_uploads_are_summarised_in_plain_words() {
        let r = SyncReport {
            files_written: 3,
            files_removed: 1,
            pushed_updates: 2,
            ..report()
        };
        assert_eq!(
            summarize(&r).as_deref(),
            Some("Updated 3 files, removed 1 file; sent 2 changes")
        );
        let r = SyncReport {
            pushed_updates: 1,
            ..report()
        };
        assert_eq!(summarize(&r).as_deref(), Some("Sent 1 change"));
    }

    #[test]
    fn held_files_and_settings_updates_are_mentioned() {
        let r = SyncReport {
            files_held: 1,
            meta_written: true,
            ..report()
        };
        assert_eq!(
            summarize(&r).as_deref(),
            Some("Updated project settings; 1 file waiting for you to save")
        );
    }

    #[test]
    fn every_halt_reason_explains_what_to_do() {
        assert!(halt_message(&HaltReason::WrongPassphrase).contains("Settings > Sync"));
        assert!(halt_message(&HaltReason::Revoked).contains("join again"));
        assert!(halt_message(&HaltReason::Fatal("boom".into())).contains("boom"));
        assert!(!halt_message(&HaltReason::ProjectMissing).is_empty());
    }

    #[test]
    fn relative_keys_are_slash_separated_and_reject_outside_paths() {
        let root = Path::new("/p");
        assert_eq!(
            relative_key(root, Path::new("/p/Draft/Ch1.md")).as_deref(),
            Some("Draft/Ch1.md")
        );
        assert_eq!(relative_key(root, Path::new("/elsewhere/x.md")), None);
    }

    #[test]
    fn the_panel_phase_follows_settings_before_pairing() {
        let state = SyncState::default();
        let mut settings = Settings::default();
        assert_eq!(state.panel_data(&settings, true).phase, SyncPanelPhase::Off);

        settings.sync_enabled = true;
        assert_eq!(
            state.panel_data(&settings, false).phase,
            SyncPanelPhase::NoProject
        );
        assert!(matches!(
            state.panel_data(&settings, true).phase,
            SyncPanelPhase::NeedsSettings(_)
        ));

        settings.sync_server_host = "sync.example.com".into();
        settings.sync_passphrase = crate::settings::SecretString("pw".into());
        assert_eq!(
            state.panel_data(&settings, true).phase,
            SyncPanelPhase::NotPaired
        );
    }

    // --- the app's side of a pass -------------------------------------------------
    //
    // These drive the real `SmaragdApp` wiring. Runners are injected (built on the
    // in-memory server with a cheap key) rather than started by `sync_ensure_running`,
    // which would derive a real key and keep its state in the OS data directory.

    use crate::project::Project;
    use crate::project::store::native_store;
    use crate::sync::crypto::cheap_test_key;
    use crate::sync::engine::{EngineConfig, SyncEngine};
    use crate::sync::fake::{MemoryServer, MemoryTransport};
    use crate::sync::state::MemoryStateStore;
    use smaragd_sync_protocol::DeviceId;
    use smaragd_sync_protocol::api::KDF_SALT_LEN;
    use uuid::Uuid;

    const VAULT: VaultId = VaultId(Uuid::from_u128(0x5eed));
    const SALT: [u8; KDF_SALT_LEN] = [3; KDF_SALT_LEN];

    fn engine(
        server: &MemoryServer,
        root: &Path,
        passphrase: &str,
    ) -> (SyncEngine, MemoryTransport) {
        let device = DeviceId(Uuid::new_v4());
        let engine = SyncEngine::open(
            EngineConfig {
                vault: VAULT,
                device,
                key: cheap_test_key(passphrase, &SALT),
                root: root.to_path_buf(),
            },
            native_store(),
            Box::new(MemoryStateStore::default()),
        )
        .unwrap();
        (engine, MemoryTransport::new(server, device))
    }

    /// An app with a fresh project open, and `SyncState` told it's already loaded that
    /// project's pairing, so `poll_sync` keeps whatever runner a test injects.
    fn app_with_project() -> (tempfile::TempDir, SmaragdApp) {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.sync.link_for = Some(project.root.clone());
        app.project = Some(project);
        (dir, app)
    }

    fn inject_runner(app: &mut SmaragdApp, server: &MemoryServer, passphrase: &str) {
        let root = app.project.as_ref().unwrap().root.clone();
        let (engine, transport) = engine(server, &root, passphrase);
        app.sync.runner = Some(SyncRunner::spawn(
            engine,
            Box::new(transport),
            Duration::from_secs(3600),
            Box::new(|| {}),
        ));
    }

    /// Runs frames of `poll_sync` until `done` holds, failing after a few seconds.
    fn poll_until(app: &mut SmaragdApp, what: &str, done: impl Fn(&SmaragdApp) -> bool) {
        let ctx = egui::Context::default();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done(app) {
            assert!(Instant::now() < deadline, "timed out waiting for: {what}");
            app.poll_sync(&ctx);
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap_or_default()
    }

    #[test]
    fn a_settings_change_from_sync_is_reloaded_so_the_next_save_keeps_it() {
        let (dir, mut app) = app_with_project();
        // Sync merges another device's settings into project.json behind the app's back.
        let mut elsewhere = Project::load_from_folder(dir.path()).unwrap();
        elsewhere.set_book_title("Merged Title".into()).unwrap();

        app.sync_handle_report(SyncReport {
            meta_written: true,
            ..report()
        });

        let project = app.project.as_ref().unwrap();
        assert_eq!(project.meta.book_title.as_deref(), Some("Merged Title"));
        // The app's next ordinary metadata save must not put the stale settings back.
        project.save_metadata().unwrap();
        let on_disk = Project::load_from_folder(dir.path()).unwrap();
        assert_eq!(on_disk.meta.book_title.as_deref(), Some("Merged Title"));
    }

    #[test]
    fn files_written_by_sync_appear_in_the_binder() {
        let (dir, mut app) = app_with_project();
        let arrived = dir.path().join("From Laptop.md");
        std::fs::write(&arrived, "Hello\n").unwrap();

        app.sync_handle_report(SyncReport {
            files_written: 1,
            ..report()
        });

        let project = app.project.as_ref().unwrap();
        assert!(project.tree.find_by_path(&arrived).is_some());
        assert_eq!(app.sync.activity.as_deref(), Some("Updated 1 file"));
    }

    #[test]
    fn a_conflict_copy_is_announced() {
        let (_dir, mut app) = app_with_project();
        app.sync_handle_report(SyncReport {
            conflict_copies: vec!["Scene (conflict copy).md".into()],
            ..report()
        });
        assert!(
            app.toasts
                .iter()
                .any(|toast| toast.message.contains("Scene (conflict copy).md"))
        );
    }

    #[test]
    fn a_document_too_large_to_upload_is_explained_once_not_every_pass() {
        let (_dir, mut app) = app_with_project();
        let stuck = || SyncReport {
            too_large: vec!["Notes/Paste.md".into()],
            ..report()
        };

        app.sync_handle_report(stuck());
        let notice = app.sync.notice.clone().expect("the panel explains it");
        assert!(notice.contains("Notes/Paste.md") && notice.contains("Remove"));
        assert_eq!(app.toasts.len(), 1);

        app.sync_handle_report(stuck());
        assert_eq!(app.toasts.len(), 1, "no new toast for the same document");
        assert!(app.sync.notice.is_some(), "still shown in the panel");

        app.sync_handle_report(report());
        assert_eq!(app.sync.notice, None);
        assert!(app.sync.too_large.is_empty());
    }

    #[test]
    fn an_open_file_with_unsaved_edits_is_held_until_saved_then_merged() {
        let server = MemoryServer::default();
        // The other device, driven by hand.
        let laptop = tempfile::tempdir().unwrap();
        let (mut laptop_engine, laptop_transport) = engine(&server, laptop.path(), "pw");
        std::fs::write(laptop.path().join("Scene.md"), "Line one.\n").unwrap();
        laptop_engine.sync_once(&laptop_transport).unwrap();

        // This device: the app, with its own runner.
        let (dir, mut app) = app_with_project();
        inject_runner(&mut app, &server, "pw");
        let scene = dir.path().join("Scene.md");
        poll_until(&mut app, "the scene to arrive", |_| {
            read(&scene) == "Line one.\n"
        });

        // The user starts editing it here, without saving...
        app.editor.open_path = Some(scene.clone());
        app.editor.dirty = true;
        poll_until(&mut app, "the held set to reach the runner", |app| {
            app.sync.held_sent.contains("Scene.md")
        });
        // ...while the laptop changes it too.
        std::fs::write(
            laptop.path().join("Scene.md"),
            "Line one.\nFrom the laptop.\n",
        )
        .unwrap();
        laptop_engine.sync_once(&laptop_transport).unwrap();
        app.sync.activity = None;
        app.sync_now();
        poll_until(&mut app, "a pass that holds the open file", |app| {
            app.sync
                .activity
                .as_deref()
                .is_some_and(|a| a.contains("waiting for you to save"))
        });
        assert_eq!(
            read(&scene),
            "Line one.\n",
            "an unsaved file was overwritten"
        );

        // Saving writes the user's version; sync then merges both edits.
        std::fs::write(&scene, "From the desktop.\nLine one.\n").unwrap();
        app.editor.dirty = false;
        app.sync_after_save();
        poll_until(&mut app, "the merged scene", |_| {
            let text = read(&scene);
            text.contains("From the desktop.") && text.contains("From the laptop.")
        });
        assert!(app.sync.held_sent.is_empty());
        laptop_engine.sync_once(&laptop_transport).unwrap();
        assert!(read(&laptop.path().join("Scene.md")).contains("From the desktop."));
    }

    #[test]
    fn a_halted_runner_stays_stopped_until_the_configuration_changes() {
        let server = MemoryServer::default();
        let laptop = tempfile::tempdir().unwrap();
        let (mut laptop_engine, laptop_transport) = engine(&server, laptop.path(), "right");
        std::fs::write(laptop.path().join("Scene.md"), "Secret.\n").unwrap();
        laptop_engine.sync_once(&laptop_transport).unwrap();

        let (dir, mut app) = app_with_project();
        app.settings.sync_enabled = true;
        app.settings.sync_server_host = "sync.example.com".into();
        app.settings.sync_passphrase = crate::settings::SecretString("wrong".into());
        app.sync.link = Some(ProjectLink {
            version: 1,
            server: app.settings.sync_server_addr().unwrap(),
            vault_id: VAULT,
            kdf_salt: SALT.to_vec(),
            key_version: 1,
        });
        app.sync.credentials = Some(DeviceCredentials {
            device_id: DeviceId(Uuid::new_v4()),
            token: "token".into(),
        });
        let signature = app.sync_signature();
        assert!(signature.is_some());
        app.sync.running_for = signature.clone();
        inject_runner(&mut app, &server, "wrong");

        poll_until(&mut app, "the runner to halt", |app| {
            matches!(app.sync.phase, Phase::Halted(_))
        });
        assert!(app.sync.runner.is_none());
        assert!(app.sync.halted_for == signature);
        assert!(app.toasts.iter().any(|t| t.message.contains("passphrase")));
        assert!(
            !dir.path().join("Scene.md").exists(),
            "wrote data it couldn't decrypt"
        );

        // Later frames with the same settings must not restart it in a loop.
        let ctx = egui::Context::default();
        for _ in 0..5 {
            app.poll_sync(&ctx);
        }
        assert!(app.sync.runner.is_none());
        assert!(matches!(app.sync.phase, Phase::Halted(_)));
    }
}
