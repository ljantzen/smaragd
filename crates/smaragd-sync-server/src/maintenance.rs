//! Housekeeping the server does by itself, so a long-running instance stays tidy without
//! anyone remembering to run anything: drop expired pairing codes, delete vaults whose
//! last device left long ago, and give unused disk space back.
//!
//! The same routine backs the admin CLI's `maintenance` command (see `admin.rs`), so a
//! cron job or a manual run behaves exactly like the background task.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

use crate::AppState;
use crate::db::{self, VaultSummary};
use crate::error::HttpError;

/// Don't bother vacuuming unless at least this much of the file is unused...
const VACUUM_MIN_FREE_BYTES: u64 = 32 * 1024 * 1024;
/// ...and it is at least this fraction of the file (one in four).
const VACUUM_MIN_FREE_DIVISOR: u64 = 4;
/// The first run happens soon after start-up rather than a whole interval later.
const FIRST_RUN_DELAY: Duration = Duration::from_secs(60);

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MaintenanceReport {
    pub expired_pairing_codes: usize,
    pub purged_vaults: Vec<VaultSummary>,
    pub vacuumed: bool,
}

impl MaintenanceReport {
    pub fn did_anything(&self) -> bool {
        self.expired_pairing_codes > 0 || !self.purged_vaults.is_empty() || self.vacuumed
    }
}

/// Whether the file has enough reclaimable space to be worth a `VACUUM`.
pub fn worth_vacuuming(fragmentation: db::Fragmentation) -> bool {
    fragmentation.free_bytes >= VACUUM_MIN_FREE_BYTES
        && fragmentation
            .free_bytes
            .saturating_mul(VACUUM_MIN_FREE_DIVISOR)
            >= fragmentation.total_bytes
}

/// One maintenance pass. `retention` is how long a vault with no devices is kept after its
/// last activity (`None` keeps them forever).
pub fn run(
    conn: &Connection,
    now: i64,
    retention: Option<Duration>,
) -> Result<MaintenanceReport, HttpError> {
    let mut report = MaintenanceReport {
        expired_pairing_codes: db::purge_expired_pairing_codes(conn, now)?,
        ..Default::default()
    };
    if let Some(retention) = retention {
        report.purged_vaults = db::purge_empty_vaults(
            conn,
            now,
            i64::try_from(retention.as_secs()).unwrap_or(i64::MAX),
        )?;
    }
    if worth_vacuuming(db::fragmentation(conn)?) {
        db::vacuum(conn)?;
        report.vacuumed = true;
    }
    Ok(report)
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Runs [`run`] every `interval`, forever (the first pass a minute after start-up).
pub async fn run_forever(state: Arc<AppState>, interval: Duration) {
    tokio::time::sleep(interval.min(FIRST_RUN_DELAY)).await;
    loop {
        let retention = state.config.empty_vault_retention;
        let now = unix_now();
        match state.db.run(move |conn| run(conn, now, retention)).await {
            Ok(report) if report.did_anything() => {
                for vault in &report.purged_vaults {
                    tracing::info!(
                        "maintenance: deleted vault {} (no devices since it was last active; {} bytes)",
                        vault.vault_id,
                        vault.bytes_used
                    );
                }
                tracing::info!(
                    "maintenance: {} expired pairing codes removed, {} vaults deleted, database {}",
                    report.expired_pairing_codes,
                    report.purged_vaults.len(),
                    if report.vacuumed {
                        "vacuumed"
                    } else {
                        "left as is"
                    }
                );
            }
            Ok(_) => tracing::debug!("maintenance: nothing to do"),
            Err(err) => tracing::warn!("maintenance failed: {err}"),
        }
        tokio::time::sleep(interval).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Fragmentation;

    const DAY: u64 = 86_400;

    #[test]
    fn vacuuming_needs_both_a_lot_of_free_space_and_a_big_fraction_of_the_file() {
        let mb: u64 = 1024 * 1024;
        let f = |free, total| Fragmentation {
            free_bytes: free * mb,
            total_bytes: total * mb,
        };
        assert!(!worth_vacuuming(f(10, 20)), "half free but tiny");
        assert!(!worth_vacuuming(f(40, 1000)), "lots free but only 4%");
        assert!(worth_vacuuming(f(40, 100)));
        assert!(worth_vacuuming(f(300, 1000)));
    }

    #[test]
    fn a_pass_removes_expired_codes_and_abandoned_vaults_and_leaves_the_rest() {
        let mut conn = db::open_in_memory().unwrap();
        let now = 1_000_000_000;
        let active = db::create_vault(&mut conn, &[1; 16], "a", now).unwrap();
        let abandoned = db::create_vault(&mut conn, &[2; 16], "b", now).unwrap();
        db::revoke_device(&conn, abandoned.vault.vault_id, abandoned.device_id, now).unwrap();
        db::create_pairing_code(&mut conn, active.vault.vault_id, now, 600).unwrap();

        let later = now + 40 * DAY as i64;
        let report = run(&conn, later, Some(Duration::from_secs(30 * DAY))).unwrap();
        assert_eq!(report.expired_pairing_codes, 1);
        assert_eq!(report.purged_vaults.len(), 1);
        assert_eq!(report.purged_vaults[0].vault_id, abandoned.vault.vault_id);
        assert!(!report.vacuumed, "a tiny database isn't worth vacuuming");
        assert!(report.did_anything());

        let ids: Vec<_> = db::list_vaults(&conn)
            .unwrap()
            .into_iter()
            .map(|v| v.vault_id)
            .collect();
        assert_eq!(ids, vec![active.vault.vault_id]);
        assert!(
            !run(&conn, later, Some(Duration::from_secs(30 * DAY)))
                .unwrap()
                .did_anything()
        );
    }

    #[test]
    fn without_a_retention_no_vault_is_ever_deleted() {
        let mut conn = db::open_in_memory().unwrap();
        let vault = db::create_vault(&mut conn, &[1; 16], "a", 1000).unwrap();
        db::revoke_device(&conn, vault.vault.vault_id, vault.device_id, 1000).unwrap();
        let report = run(&conn, 1000 + 10_000 * DAY as i64, None).unwrap();
        assert!(report.purged_vaults.is_empty());
        assert_eq!(db::list_vaults(&conn).unwrap().len(), 1);
    }
}
