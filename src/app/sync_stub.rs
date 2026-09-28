//! The browser build's no-op twin of `sync.rs`: sync needs sockets, threads and the OS
//! data directory, none of which exist on `wasm32-unknown-unknown` (a `fetch`-based
//! transport is future work). Keeps the same entry points so the rest of the app needs no
//! `cfg` scattered through it.

use super::SmaragdApp;
use crate::settings::Settings;
use crate::ui::sync_panel::{SyncPanelData, SyncPanelEvent, SyncPanelPhase};

#[derive(Default)]
pub(super) struct SyncState;

impl SyncState {
    pub(super) fn panel_data<'a>(
        &'a self,
        _settings: &'a Settings,
        _has_project: bool,
    ) -> SyncPanelData<'a> {
        SyncPanelData {
            phase: SyncPanelPhase::Off,
            busy: false,
            server: None,
            activity: None,
            ticket: None,
            devices: None,
            own_device: None,
            now_unix: 0,
            notice: None,
        }
    }
}

impl SmaragdApp {
    pub(super) fn poll_sync(&mut self, _ctx: &egui::Context) {}
    pub(super) fn sync_after_save(&mut self) {}
    pub(super) fn handle_sync_panel_event(&mut self, _ctx: &egui::Context, _event: SyncPanelEvent) {
    }
    pub(super) fn sync_join_with_ticket(&mut self, _ctx: &egui::Context, _pasted: &str) {}
    pub(super) fn sync_create_vault_with_token(&mut self, _ctx: &egui::Context, _token: &str) {}
    pub(super) fn sync_is_running(&self) -> bool {
        false
    }
}
