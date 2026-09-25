//! The `smaragd-sync-server admin ...` command line, for the operator.
//!
//! It works directly on the database in `SMARAGD_SYNC_DATA_DIR`, so it can be run on
//! the host or with `docker exec <container> smaragd-sync-server admin list` while the
//! server is running (SQLite in WAL mode allows it; `vacuum` briefly needs the database
//! to itself, so it may ask you to retry).
//!
//! Everything destructive is opt-in: `delete-vault` and `purge-empty` only *show* what
//! they would delete unless given `--yes`.

use std::io::Write;
use std::path::PathBuf;
use std::str::FromStr;

use smaragd_sync_protocol::VaultId;

use crate::config::Config;
use crate::db::{self, VaultSummary};
use crate::maintenance;

pub const USAGE: &str = "\
Usage: smaragd-sync-server admin <command>

Commands:
  list                         Show every vault: devices, documents, size, last activity
  delete-vault <id> --yes      Permanently delete one vault and all its data
  purge-empty [--days N] [--yes]
                               Delete vaults that have no devices left and have been
                               inactive for N days (default: the server's retention
                               setting, 30). Without --yes it only lists them.
  vacuum                       Give the database's unused disk space back
  maintenance                  Run one background-maintenance pass now

The data directory comes from SMARAGD_SYNC_DATA_DIR, like the server itself.";

pub fn database_path(config: &Config) -> PathBuf {
    config.data_dir.join("sync.sqlite3")
}

/// `1234567` -> `1.2 MiB`.
pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Unix seconds -> `YYYY-MM-DD` (UTC), without pulling in a date library.
pub fn ymd(unix_seconds: i64) -> String {
    let days = unix_seconds.div_euclid(86_400);
    // Howard Hinnant's civil-from-days algorithm.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    format!("{year:04}-{month:02}-{day:02}")
}

fn print_vaults(out: &mut dyn Write, vaults: &[VaultSummary]) -> std::io::Result<()> {
    writeln!(
        out,
        "{:<36}  {:>7}  {:>5}  {:>10}  {:<10}  {:<10}",
        "VAULT", "DEVICES", "DOCS", "SIZE", "CREATED", "LAST ACTIVE"
    )?;
    for vault in vaults {
        writeln!(
            out,
            "{:<36}  {:>7}  {:>5}  {:>10}  {:<10}  {:<10}",
            vault.vault_id,
            vault.devices,
            vault.docs,
            human_bytes(vault.bytes_used),
            ymd(vault.created_at),
            ymd(vault.last_active)
        )?;
    }
    Ok(())
}

fn io_err(err: std::io::Error) -> String {
    format!("writing output: {err}")
}

/// Runs one admin command. `now` is passed in so tests are deterministic.
pub fn run(args: &[String], config: &Config, now: i64, out: &mut dyn Write) -> Result<(), String> {
    let Some(command) = args.first() else {
        return Err(USAGE.to_string());
    };
    let path = database_path(config);
    if !path.exists() {
        return Err(format!(
            "no database at {} — is SMARAGD_SYNC_DATA_DIR pointing at the server's data?",
            path.display()
        ));
    }
    let conn = db::open(&path).map_err(|err| format!("opening {}: {err}", path.display()))?;
    let flag = |name: &str| args.iter().any(|arg| arg == name);
    let db_err = |err: crate::error::HttpError| err.to_string();

    match command.as_str() {
        "list" => {
            let vaults = db::list_vaults(&conn).map_err(db_err)?;
            print_vaults(out, &vaults).map_err(io_err)?;
            let total: u64 = vaults.iter().map(|v| v.bytes_used).sum();
            let frag = db::fragmentation(&conn).map_err(db_err)?;
            writeln!(
                out,
                "{} vault(s), {} of ciphertext; database file {} ({} reclaimable)",
                vaults.len(),
                human_bytes(total),
                human_bytes(frag.total_bytes),
                human_bytes(frag.free_bytes)
            )
            .map_err(io_err)
        }
        "delete-vault" => {
            let id = args
                .get(1)
                .filter(|arg| !arg.starts_with("--"))
                .ok_or("delete-vault needs a vault id (see `admin list`)")?;
            let id = VaultId::from_str(id).map_err(|_| format!("{id:?} is not a vault id"))?;
            let vault = db::list_vaults(&conn)
                .map_err(db_err)?
                .into_iter()
                .find(|v| v.vault_id == id)
                .ok_or_else(|| format!("no vault {id}"))?;
            if !flag("--yes") {
                print_vaults(out, std::slice::from_ref(&vault)).map_err(io_err)?;
                return Err(format!(
                    "this would permanently delete the vault above ({} device(s), {}). Run again with --yes to do it.",
                    vault.devices,
                    human_bytes(vault.bytes_used)
                ));
            }
            db::delete_vault(&conn, id).map_err(db_err)?;
            writeln!(
                out,
                "Deleted vault {id} ({}).",
                human_bytes(vault.bytes_used)
            )
            .map_err(io_err)
        }
        "purge-empty" => {
            let days = match args.iter().position(|arg| arg == "--days") {
                Some(i) => args
                    .get(i + 1)
                    .and_then(|n| n.parse::<u64>().ok())
                    .ok_or("--days needs a whole number")?,
                None => config
                    .empty_vault_retention
                    .map_or(30, |d| d.as_secs() / 86_400),
            };
            let secs = i64::try_from(days.saturating_mul(86_400)).unwrap_or(i64::MAX);
            if flag("--yes") {
                let purged = db::purge_empty_vaults(&conn, now, secs).map_err(db_err)?;
                if !purged.is_empty() {
                    print_vaults(out, &purged).map_err(io_err)?;
                }
                writeln!(out, "Deleted {} vault(s).", purged.len()).map_err(io_err)
            } else {
                let found = db::find_empty_vaults(&conn, now, secs).map_err(db_err)?;
                if found.is_empty() {
                    return writeln!(
                        out,
                        "No vaults have been without devices and inactive for {days} day(s)."
                    )
                    .map_err(io_err);
                }
                print_vaults(out, &found).map_err(io_err)?;
                writeln!(
                    out,
                    "{} vault(s) would be deleted (no devices, inactive for {days}+ days). Run again with --yes to do it.",
                    found.len()
                )
                .map_err(io_err)
            }
        }
        "vacuum" => {
            let before = db::fragmentation(&conn).map_err(db_err)?;
            db::vacuum(&conn).map_err(|err| {
                format!("{err} — if the server is busy, try again in a moment or stop it first")
            })?;
            let after = db::fragmentation(&conn).map_err(db_err)?;
            writeln!(
                out,
                "Database file: {} -> {}.",
                human_bytes(before.total_bytes),
                human_bytes(after.total_bytes)
            )
            .map_err(io_err)
        }
        "maintenance" => {
            let report =
                maintenance::run(&conn, now, config.empty_vault_retention).map_err(db_err)?;
            writeln!(
                out,
                "{} expired pairing code(s) removed, {} vault(s) deleted, database {}.",
                report.expired_pairing_codes,
                report.purged_vaults.len(),
                if report.vacuumed {
                    "vacuumed"
                } else {
                    "left as is (little to reclaim)"
                }
            )
            .map_err(io_err)
        }
        "help" | "--help" | "-h" => writeln!(out, "{USAGE}").map_err(io_err),
        other => Err(format!("unknown command {other:?}\n\n{USAGE}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000; // 2027-01-15
    const DAY: i64 = 86_400;

    fn config(dir: &std::path::Path) -> Config {
        Config::from_lookup(|key| {
            (key == "SMARAGD_SYNC_DATA_DIR").then(|| dir.to_string_lossy().into_owned())
        })
        .unwrap()
    }

    /// A data dir with one live vault and one abandoned for 60 days.
    fn populated() -> (tempfile::TempDir, VaultId, VaultId) {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = db::open(&dir.path().join("sync.sqlite3")).unwrap();
        let live = db::create_vault(&mut conn, &[1; 16], "laptop", NOW - 100 * DAY).unwrap();
        let dead = db::create_vault(&mut conn, &[2; 16], "old", NOW - 100 * DAY).unwrap();
        db::push_update(
            &mut conn,
            dead.vault.vault_id,
            smaragd_sync_protocol::DocId(uuid::Uuid::from_u128(5)),
            dead.device_id,
            &[9; 2048],
            1 << 30,
            NOW - 90 * DAY,
        )
        .unwrap();
        db::revoke_device(&conn, dead.vault.vault_id, dead.device_id, NOW - 60 * DAY).unwrap();
        (dir, live.vault.vault_id, dead.vault.vault_id)
    }

    fn run_cli(dir: &std::path::Path, args: &[&str]) -> (Result<(), String>, String) {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let mut out = Vec::new();
        let result = run(&args, &config(dir), NOW, &mut out);
        (result, String::from_utf8(out).unwrap())
    }

    fn vault_ids(dir: &std::path::Path) -> Vec<VaultId> {
        let conn = db::open(&dir.join("sync.sqlite3")).unwrap();
        db::list_vaults(&conn)
            .unwrap()
            .into_iter()
            .map(|v| v.vault_id)
            .collect()
    }

    #[test]
    fn list_shows_each_vault_with_devices_size_and_dates() {
        let (dir, live, dead) = populated();
        let (result, out) = run_cli(dir.path(), &["list"]);
        result.unwrap();
        assert!(
            out.contains(&live.to_string()) && out.contains(&dead.to_string()),
            "{out}"
        );
        assert!(out.contains("2.0 KiB"), "{out}");
        assert!(
            out.contains("2026-10-07"),
            "created 100 days before NOW: {out}"
        );
        assert!(out.contains("2 vault(s)"), "{out}");
    }

    #[test]
    fn delete_vault_needs_yes_and_then_removes_only_that_vault() {
        let (dir, live, dead) = populated();
        let (result, out) = run_cli(dir.path(), &["delete-vault", &dead.to_string()]);
        assert!(result.unwrap_err().contains("--yes"));
        assert!(
            out.contains(&dead.to_string()),
            "it shows what it would delete"
        );
        assert_eq!(
            vault_ids(dir.path()).len(),
            2,
            "nothing deleted without --yes"
        );

        let (result, out) = run_cli(dir.path(), &["delete-vault", &dead.to_string(), "--yes"]);
        result.unwrap();
        assert!(out.contains("Deleted vault"));
        assert_eq!(vault_ids(dir.path()), vec![live]);
    }

    #[test]
    fn delete_vault_rejects_bad_ids_and_unknown_vaults() {
        let (dir, _, _) = populated();
        assert!(run_cli(dir.path(), &["delete-vault"]).0.is_err());
        assert!(
            run_cli(dir.path(), &["delete-vault", "nonsense", "--yes"])
                .0
                .is_err()
        );
        let missing = VaultId(uuid::Uuid::from_u128(999)).to_string();
        assert!(
            run_cli(dir.path(), &["delete-vault", &missing, "--yes"])
                .0
                .unwrap_err()
                .contains("no vault")
        );
    }

    #[test]
    fn purge_empty_lists_first_and_deletes_only_with_yes() {
        let (dir, live, dead) = populated();
        let (result, out) = run_cli(dir.path(), &["purge-empty", "--days", "30"]);
        result.unwrap();
        assert!(
            out.contains(&dead.to_string()) && out.contains("would be deleted"),
            "{out}"
        );
        assert_eq!(vault_ids(dir.path()).len(), 2);

        let (result, out) = run_cli(dir.path(), &["purge-empty", "--days", "90"]);
        result.unwrap();
        assert!(
            out.contains("No vaults"),
            "60 days idle is under a 90-day retention: {out}"
        );

        let (result, out) = run_cli(dir.path(), &["purge-empty", "--days", "30", "--yes"]);
        result.unwrap();
        assert!(out.contains("Deleted 1 vault(s)"), "{out}");
        assert_eq!(vault_ids(dir.path()), vec![live]);
    }

    #[test]
    fn maintenance_and_vacuum_run_cleanly() {
        let (dir, live, _) = populated();
        let (result, out) = run_cli(dir.path(), &["maintenance"]);
        result.unwrap();
        assert!(
            out.contains("1 vault(s) deleted"),
            "default 30-day retention: {out}"
        );
        assert_eq!(vault_ids(dir.path()), vec![live]);
        let (result, out) = run_cli(dir.path(), &["vacuum"]);
        result.unwrap();
        assert!(out.contains("Database file"), "{out}");
    }

    #[test]
    fn a_missing_database_and_unknown_commands_are_explained() {
        let empty = tempfile::tempdir().unwrap();
        assert!(
            run_cli(empty.path(), &["list"])
                .0
                .unwrap_err()
                .contains("no database")
        );
        let (dir, _, _) = populated();
        assert!(
            run_cli(dir.path(), &["frobnicate"])
                .0
                .unwrap_err()
                .contains("Usage")
        );
        assert!(run_cli(dir.path(), &[]).0.unwrap_err().contains("Usage"));
        assert!(run_cli(dir.path(), &["help"]).0.is_ok());
    }

    #[test]
    fn dates_and_sizes_are_formatted_for_humans() {
        assert_eq!(ymd(0), "1970-01-01");
        assert_eq!(ymd(951_782_400), "2000-02-29");
        assert_eq!(ymd(NOW), "2027-01-15");
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(1023), "1023 B");
        assert_eq!(human_bytes(1536), "1.5 KiB");
        assert_eq!(human_bytes(5 * 1024 * 1024 * 1024), "5.0 GiB");
    }
}
