//! The Sync dock tab: whether this project is syncing, when it last did, the devices in
//! its vault, and the pairing controls.
//!
//! Pure rendering, like `collab_panel`: the caller (`app::sync`) derives a
//! [`SyncPanelData`] from its state each frame and handles the returned
//! [`SyncPanelEvent`]. Nothing here touches the network or the engine.

use smaragd_sync_protocol::DeviceId;
use smaragd_sync_protocol::api::DeviceInfo;

/// Where sync stands for the current project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncPanelPhase<'a> {
    /// Sync is switched off in Settings.
    Off,
    NoProject,
    /// Settings > Sync isn't filled in yet; the string says what's missing.
    NeedsSettings(&'a str),
    /// Sync is on, but this project isn't paired with a vault.
    NotPaired,
    /// The engine is starting (deriving the key takes about a second).
    Starting,
    Syncing,
    /// The last pass succeeded; the string is when, already phrased ("2 min ago").
    UpToDate {
        last_synced: &'a str,
    },
    /// The server can't be reached; edits queue and the next pass retries.
    Offline {
        reason: &'a str,
    },
    /// Sync stopped and won't retry by itself.
    Halted {
        message: &'a str,
    },
}

/// Everything the panel shows, borrowed from the app's sync state.
pub struct SyncPanelData<'a> {
    pub phase: SyncPanelPhase<'a>,
    /// A one-off server request (create/join/list/revoke...) is in flight.
    pub busy: bool,
    /// `host:port` of the vault's server, for display.
    pub server: Option<&'a str>,
    /// What the last pass did ("Updated 3 files, 1 conflict copy").
    pub activity: Option<&'a str>,
    /// The latest pairing ticket to share, if one was made.
    pub ticket: Option<&'a str>,
    pub devices: Option<&'a [DeviceInfo]>,
    pub own_device: Option<DeviceId>,
    /// Seconds since the Unix epoch, to phrase device "last seen" times.
    pub now_unix: u64,
    /// The most recent failure of a one-off request, if any.
    pub notice: Option<&'a str>,
    /// A pasted pairing ticket waiting for the user to confirm its server.
    pub pending_join: Option<PendingJoin<'a>>,
}

/// Where a pasted ticket would connect, shown before anything is sent: a ticket is
/// whatever someone pasted, and joining uploads this project to its server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingJoin<'a> {
    /// `host:port[/path]`, as the ticket names it.
    pub server: &'a str,
    pub plain_http: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncPanelEvent {
    OpenSettings,
    /// Create a new vault for this project (asks for the admin token, if needed).
    CreateVault,
    /// Join an existing vault with a pairing ticket.
    JoinVault,
    /// Go ahead with the pasted ticket's server (see [`PendingJoin`]).
    ConfirmJoin,
    CancelJoin,
    SyncNow,
    /// Start again after a halt.
    Retry,
    /// Make a pairing ticket for another device.
    MakeTicket,
    RefreshDevices,
    Revoke(DeviceId),
    /// Stop syncing this project and remove this device from the vault.
    LeaveVault,
}

/// "just now", "3 min ago", "2 h ago", "5 days ago".
pub fn humanize_age(seconds: u64) -> String {
    match seconds {
        0..=44 => "just now".to_string(),
        45..=3_599 => format!("{} min ago", (seconds + 30) / 60),
        3_600..=86_399 => format!("{} h ago", (seconds + 1_800) / 3_600),
        _ => format!("{} days ago", (seconds + 43_200) / 86_400),
    }
}

pub fn show(ui: &mut egui::Ui, data: &SyncPanelData) -> Option<SyncPanelEvent> {
    let mut event = None;
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.add_space(8.0);
        match data.phase {
            SyncPanelPhase::Off => {
                ui.label("Sync is turned off.");
                ui.weak("Turn it on, and set your server and passphrase, in Settings > Sync.");
                if ui.button("Open Settings").clicked() {
                    event = Some(SyncPanelEvent::OpenSettings);
                }
            }
            SyncPanelPhase::NoProject => {
                ui.label("Open a project to sync it.");
            }
            SyncPanelPhase::NeedsSettings(problem) => {
                ui.label(problem);
                if ui.button("Open Settings").clicked() {
                    event = Some(SyncPanelEvent::OpenSettings);
                }
            }
            SyncPanelPhase::NotPaired if data.pending_join.is_some() => {
                if let Some(e) = show_pending_join(ui, data) {
                    event = Some(e);
                }
            }
            SyncPanelPhase::NotPaired => {
                ui.label("This project isn't syncing yet.");
                ui.weak(
                    "Create a vault to start syncing it, or join a vault you already have \
                     on another device with a pairing ticket.",
                );
                ui.add_space(8.0);
                ui.add_enabled_ui(!data.busy, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("Create Vault…").clicked() {
                            event = Some(SyncPanelEvent::CreateVault);
                        }
                        if ui.button("Join Vault…").clicked() {
                            event = Some(SyncPanelEvent::JoinVault);
                        }
                    });
                });
                if data.busy {
                    ui.weak("Contacting the server…");
                }
            }
            paired => {
                if let Some(e) = show_paired(ui, data, paired) {
                    event = Some(e);
                }
            }
        }
        if let Some(notice) = data.notice {
            ui.add_space(8.0);
            ui.colored_label(ui.visuals().warn_fg_color, notice);
        }
    });
    event
}

fn show_pending_join(ui: &mut egui::Ui, data: &SyncPanelData) -> Option<SyncPanelEvent> {
    let pending = data.pending_join?;
    let mut event = None;
    ui.label("Join the vault on this server?");
    ui.strong(pending.server);
    if pending.plain_http {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            "This ticket uses plain HTTP: this device's token would travel unencrypted. \
             Your text stays encrypted, but only continue on a network you trust.",
        );
    }
    ui.weak(
        "Joining uploads this project to that server, encrypted with your passphrase. \
         Only continue if the ticket came from one of your own devices and this is your \
         server.",
    );
    ui.add_space(8.0);
    ui.add_enabled_ui(!data.busy, |ui| {
        ui.horizontal(|ui| {
            if ui.button("Join").clicked() {
                event = Some(SyncPanelEvent::ConfirmJoin);
            }
            if ui.button("Cancel").clicked() {
                event = Some(SyncPanelEvent::CancelJoin);
            }
        });
    });
    if data.busy {
        ui.weak("Contacting the server…");
    }
    event
}

fn show_paired(
    ui: &mut egui::Ui,
    data: &SyncPanelData,
    phase: SyncPanelPhase,
) -> Option<SyncPanelEvent> {
    let mut event = None;
    let visuals = ui.visuals();
    let (dot_color, headline) = match phase {
        SyncPanelPhase::Starting => (visuals.weak_text_color(), "Starting…".to_string()),
        SyncPanelPhase::Syncing => (visuals.weak_text_color(), "Syncing…".to_string()),
        SyncPanelPhase::UpToDate { last_synced } => (
            egui::Color32::from_rgb(0x3c, 0xb0, 0x5a),
            format!("Up to date — synced {last_synced}"),
        ),
        SyncPanelPhase::Offline { .. } => (
            visuals.warn_fg_color,
            "Offline — changes are saved and will sync when the server is reachable".to_string(),
        ),
        SyncPanelPhase::Halted { message } => (visuals.error_fg_color, message.to_string()),
        _ => return None,
    };
    ui.horizontal_wrapped(|ui| {
        ui.colored_label(dot_color, "●");
        ui.label(headline);
    });
    if let SyncPanelPhase::Offline { reason } = phase {
        ui.weak(reason);
    }
    if let Some(server) = data.server {
        ui.weak(format!("Server: {server}"));
    }
    if let Some(activity) = data.activity {
        ui.weak(activity);
    }
    ui.add_space(6.0);

    ui.horizontal(|ui| match phase {
        SyncPanelPhase::Halted { .. } => {
            if ui.button("Retry").clicked() {
                event = Some(SyncPanelEvent::Retry);
            }
            if ui.button("Open Settings").clicked() {
                event = Some(SyncPanelEvent::OpenSettings);
            }
        }
        _ => {
            let idle = matches!(
                phase,
                SyncPanelPhase::UpToDate { .. } | SyncPanelPhase::Offline { .. }
            );
            if ui
                .add_enabled(idle, egui::Button::new("Sync Now"))
                .clicked()
            {
                event = Some(SyncPanelEvent::SyncNow);
            }
        }
    });

    ui.add_space(10.0);
    ui.separator();
    ui.weak("Also syncing images, PDFs and other files? Configure in Project Settings > Sync.");

    ui.add_space(10.0);
    ui.separator();
    ui.strong("Add another device");
    ui.weak(
        "Make a pairing ticket, paste it into Smaragd on the other device (Sync panel > \
         Join Vault…), and use the same passphrase there. A ticket works once and expires \
         after 10 minutes.",
    );
    ui.add_enabled_ui(!data.busy, |ui| {
        if ui.button("Make Pairing Ticket").clicked() {
            event = Some(SyncPanelEvent::MakeTicket);
        }
    });
    if let Some(ticket) = data.ticket {
        let mut shown = ticket.to_string();
        ui.add(
            egui::TextEdit::singleline(&mut shown)
                .desired_width(f32::INFINITY)
                .interactive(false),
        );
        if ui.button("Copy").clicked() {
            ui.ctx().copy_text(ticket.to_string());
        }
    }

    ui.add_space(10.0);
    ui.separator();
    ui.horizontal(|ui| {
        ui.strong("Devices");
        ui.add_enabled_ui(!data.busy, |ui| {
            if ui.small_button("Refresh").clicked() {
                event = Some(SyncPanelEvent::RefreshDevices);
            }
        });
    });
    match data.devices {
        None => {
            ui.weak("Press Refresh to list the devices in this vault.");
        }
        Some([]) => {
            ui.weak("No devices found.");
        }
        Some(devices) => {
            for device in devices {
                let is_self = Some(device.device_id) == data.own_device;
                ui.horizontal(|ui| {
                    let seen = device.last_seen_unix.map_or_else(
                        || "never seen".to_string(),
                        |at| humanize_age(data.now_unix.saturating_sub(at)),
                    );
                    let name = if is_self {
                        format!("{} (this device)", device.name)
                    } else {
                        device.name.clone()
                    };
                    ui.label(name);
                    ui.weak(seen);
                    if let Some(added_by) = paired_by_label(device, devices) {
                        ui.weak(added_by);
                    }
                    if !is_self
                        && ui
                            .add_enabled(!data.busy, egui::Button::new("Revoke").small())
                            .on_hover_text("This device stops syncing immediately")
                            .clicked()
                    {
                        event = Some(SyncPanelEvent::Revoke(device.device_id));
                    }
                });
            }
        }
    }

    ui.add_space(10.0);
    ui.collapsing("Stop syncing this project", |ui| {
        ui.weak(
            "Removes this device from the vault and stops syncing this project. Your files \
             stay exactly as they are here, and the other devices keep their copies.",
        );
        ui.add_enabled_ui(!data.busy, |ui| {
            if ui.button("Stop Syncing This Project").clicked() {
                event = Some(SyncPanelEvent::LeaveVault);
            }
        });
    });
    event
}

/// Which device let `device` into the vault, for spotting one added with a stolen
/// token. `None` for the vault's first device (and when the server doesn't say).
fn paired_by_label(device: &DeviceInfo, devices: &[DeviceInfo]) -> Option<String> {
    let by = device.paired_by?;
    Some(match devices.iter().find(|other| other.device_id == by) {
        Some(pairer) => format!("added by {}", pairer.name),
        None => "added by a removed device".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::egui_test_support::run_ui_and_discard;
    use uuid::Uuid;

    #[test]
    fn ages_are_phrased_for_humans() {
        assert_eq!(humanize_age(0), "just now");
        assert_eq!(humanize_age(44), "just now");
        assert_eq!(humanize_age(90), "2 min ago");
        assert_eq!(humanize_age(3_000), "50 min ago");
        assert_eq!(humanize_age(7_200), "2 h ago");
        assert_eq!(humanize_age(3 * 86_400), "3 days ago");
    }

    fn data<'a>(phase: SyncPanelPhase<'a>, devices: Option<&'a [DeviceInfo]>) -> SyncPanelData<'a> {
        SyncPanelData {
            phase,
            busy: false,
            server: Some("sync.example.com:443"),
            activity: Some("Updated 2 files"),
            ticket: Some("3Kq9ticket"),
            devices,
            own_device: Some(DeviceId(Uuid::from_u128(1))),
            now_unix: 1_000_000,
            notice: Some("Couldn't reach the server"),
            pending_join: None,
        }
    }

    fn device(n: u128, name: &str, seen: Option<u64>) -> DeviceInfo {
        DeviceInfo {
            device_id: DeviceId(Uuid::from_u128(n)),
            name: name.into(),
            created_at_unix: 0,
            last_seen_unix: seen,
            paired_by: None,
        }
    }

    #[test]
    fn each_device_says_which_device_added_it() {
        let laptop = device(1, "laptop", None);
        let mut phone = device(2, "phone", None);
        phone.paired_by = Some(laptop.device_id);
        let mut tablet = device(3, "tablet", None);
        tablet.paired_by = Some(DeviceId(Uuid::from_u128(99)));
        let all = [laptop.clone(), phone.clone(), tablet.clone()];

        assert_eq!(paired_by_label(&laptop, &all), None);
        assert_eq!(
            paired_by_label(&phone, &all).as_deref(),
            Some("added by laptop")
        );
        assert_eq!(
            paired_by_label(&tablet, &all).as_deref(),
            Some("added by a removed device")
        );
    }

    #[test]
    fn a_pending_ticket_renders_with_and_without_the_plain_http_warning() {
        let ctx = egui::Context::default();
        for plain_http in [false, true] {
            let mut panel = data(SyncPanelPhase::NotPaired, None);
            panel.pending_join = Some(PendingJoin {
                server: "sync.example.com:443",
                plain_http,
            });
            run_ui_and_discard(&ctx, egui::RawInput::default(), |ui| {
                show(ui, &panel);
            });
        }
    }

    #[test]
    fn every_phase_renders_without_panicking() {
        let devices = [device(1, "laptop", Some(999_990)), device(2, "phone", None)];
        let phases = [
            SyncPanelPhase::Off,
            SyncPanelPhase::NoProject,
            SyncPanelPhase::NeedsSettings("Enter the server."),
            SyncPanelPhase::NotPaired,
            SyncPanelPhase::Starting,
            SyncPanelPhase::Syncing,
            SyncPanelPhase::UpToDate {
                last_synced: "just now",
            },
            SyncPanelPhase::Offline {
                reason: "connection refused",
            },
            SyncPanelPhase::Halted {
                message: "The passphrase doesn't match.",
            },
        ];
        let ctx = egui::Context::default();
        for phase in phases {
            for listing in [None, Some(&devices[..]), Some(&[][..])] {
                run_ui_and_discard(&ctx, egui::RawInput::default(), |ui| {
                    show(ui, &data(phase, listing));
                });
            }
        }
    }

    #[test]
    fn an_idle_panel_reports_no_event() {
        let ctx = egui::Context::default();
        let mut got = Some(SyncPanelEvent::SyncNow);
        run_ui_and_discard(&ctx, egui::RawInput::default(), |ui| {
            got = show(ui, &data(SyncPanelPhase::NotPaired, None));
        });
        assert_eq!(got, None);
    }
}
