//! Server configuration, read from environment variables (the natural fit for a
//! Docker image; there is deliberately no config file).

use std::path::PathBuf;
use std::time::Duration;

const DEFAULT_LISTEN_ADDR: &str = "0.0.0.0:8080";
const DEFAULT_DATA_DIR: &str = "./data";
const DEFAULT_QUOTA_MB: u64 = 1024;
const DEFAULT_MAINTENANCE_HOURS: u64 = 6;
const DEFAULT_EMPTY_VAULT_RETENTION_DAYS: u64 = 30;

#[derive(Debug, Clone)]
pub struct Config {
    /// `SMARAGD_SYNC_LISTEN_ADDR` — where to listen (plain HTTP; put a TLS-terminating
    /// reverse proxy in front for anything beyond localhost).
    pub listen_addr: String,
    /// `SMARAGD_SYNC_DATA_DIR` — the one directory holding all state (mount a volume).
    pub data_dir: PathBuf,
    /// `SMARAGD_SYNC_ALLOW_OPEN_REGISTRATION` — whether anyone who can reach the
    /// server may create a vault. Off by default.
    pub allow_open_registration: bool,
    /// `SMARAGD_SYNC_ADMIN_TOKEN` — lets the holder create vaults even when open
    /// registration is off.
    pub admin_token: Option<String>,
    /// `SMARAGD_SYNC_VAULT_QUOTA_MB` — per-vault storage cap for ciphertext.
    pub vault_quota_bytes: u64,
    /// `SMARAGD_SYNC_MAINTENANCE_INTERVAL_HOURS` — how often the background maintenance
    /// task runs (expired pairing codes, abandoned vaults, reclaiming disk space).
    /// `0` turns it off (the admin CLI can still run it by hand).
    pub maintenance_interval: Option<Duration>,
    /// `SMARAGD_SYNC_EMPTY_VAULT_RETENTION_DAYS` — a vault with no devices left is
    /// deleted this long after its last activity. `0` keeps such vaults forever.
    pub empty_vault_retention: Option<Duration>,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let non_empty = |key: &str| get(key).filter(|value| !value.trim().is_empty());
        let allow_open_registration = match non_empty("SMARAGD_SYNC_ALLOW_OPEN_REGISTRATION") {
            None => false,
            Some(value) => match value.trim().to_ascii_lowercase().as_str() {
                "1" | "true" | "yes" | "on" => true,
                "0" | "false" | "no" | "off" => false,
                other => {
                    return Err(format!(
                        "SMARAGD_SYNC_ALLOW_OPEN_REGISTRATION must be true or false, got {other:?}"
                    ));
                }
            },
        };
        let quota_mb = match non_empty("SMARAGD_SYNC_VAULT_QUOTA_MB") {
            None => DEFAULT_QUOTA_MB,
            Some(value) => value.trim().parse::<u64>().map_err(|_| {
                format!("SMARAGD_SYNC_VAULT_QUOTA_MB must be a whole number, got {value:?}")
            })?,
        };
        let duration =
            |key: &str, default: u64, unit_secs: u64| -> Result<Option<Duration>, String> {
                let value = match non_empty(key) {
                    None => default,
                    Some(text) => text
                        .trim()
                        .parse::<u64>()
                        .map_err(|_| format!("{key} must be a whole number, got {text:?}"))?,
                };
                Ok((value > 0).then(|| Duration::from_secs(value.saturating_mul(unit_secs))))
            };
        let maintenance_interval = duration(
            "SMARAGD_SYNC_MAINTENANCE_INTERVAL_HOURS",
            DEFAULT_MAINTENANCE_HOURS,
            3600,
        )?;
        let empty_vault_retention = duration(
            "SMARAGD_SYNC_EMPTY_VAULT_RETENTION_DAYS",
            DEFAULT_EMPTY_VAULT_RETENTION_DAYS,
            86_400,
        )?;
        Ok(Self {
            listen_addr: non_empty("SMARAGD_SYNC_LISTEN_ADDR")
                .unwrap_or_else(|| DEFAULT_LISTEN_ADDR.to_string()),
            data_dir: PathBuf::from(
                non_empty("SMARAGD_SYNC_DATA_DIR").unwrap_or_else(|| DEFAULT_DATA_DIR.to_string()),
            ),
            allow_open_registration,
            admin_token: non_empty("SMARAGD_SYNC_ADMIN_TOKEN"),
            vault_quota_bytes: quota_mb.saturating_mul(1024 * 1024),
            maintenance_interval,
            empty_vault_retention,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |key| map.get(key).cloned()
    }

    #[test]
    fn defaults_are_closed_registration_on_8080() {
        let cfg = Config::from_lookup(lookup(&[])).unwrap();
        assert_eq!(cfg.listen_addr, "0.0.0.0:8080");
        assert!(!cfg.allow_open_registration);
        assert_eq!(cfg.admin_token, None);
        assert_eq!(cfg.vault_quota_bytes, 1024 * 1024 * 1024);
        assert_eq!(
            cfg.maintenance_interval,
            Some(Duration::from_secs(6 * 3600))
        );
        assert_eq!(
            cfg.empty_vault_retention,
            Some(Duration::from_secs(30 * 86_400))
        );
    }

    #[test]
    fn maintenance_settings_are_in_hours_and_days_and_zero_turns_them_off() {
        let cfg = Config::from_lookup(lookup(&[
            ("SMARAGD_SYNC_MAINTENANCE_INTERVAL_HOURS", "12"),
            ("SMARAGD_SYNC_EMPTY_VAULT_RETENTION_DAYS", "7"),
        ]))
        .unwrap();
        assert_eq!(
            cfg.maintenance_interval,
            Some(Duration::from_secs(12 * 3600))
        );
        assert_eq!(
            cfg.empty_vault_retention,
            Some(Duration::from_secs(7 * 86_400))
        );

        let off = Config::from_lookup(lookup(&[
            ("SMARAGD_SYNC_MAINTENANCE_INTERVAL_HOURS", "0"),
            ("SMARAGD_SYNC_EMPTY_VAULT_RETENTION_DAYS", "0"),
        ]))
        .unwrap();
        assert_eq!(off.maintenance_interval, None);
        assert_eq!(off.empty_vault_retention, None);
        assert!(
            Config::from_lookup(lookup(&[(
                "SMARAGD_SYNC_EMPTY_VAULT_RETENTION_DAYS",
                "soon"
            )]))
            .is_err()
        );
    }

    #[test]
    fn every_variable_is_honoured() {
        let cfg = Config::from_lookup(lookup(&[
            ("SMARAGD_SYNC_LISTEN_ADDR", "127.0.0.1:9000"),
            ("SMARAGD_SYNC_DATA_DIR", "/data"),
            ("SMARAGD_SYNC_ALLOW_OPEN_REGISTRATION", "TRUE"),
            ("SMARAGD_SYNC_ADMIN_TOKEN", "s3cret"),
            ("SMARAGD_SYNC_VAULT_QUOTA_MB", "5"),
        ]))
        .unwrap();
        assert_eq!(cfg.listen_addr, "127.0.0.1:9000");
        assert_eq!(cfg.data_dir, PathBuf::from("/data"));
        assert!(cfg.allow_open_registration);
        assert_eq!(cfg.admin_token.as_deref(), Some("s3cret"));
        assert_eq!(cfg.vault_quota_bytes, 5 * 1024 * 1024);
    }

    #[test]
    fn blank_values_count_as_unset_and_garbage_is_rejected() {
        let cfg = Config::from_lookup(lookup(&[("SMARAGD_SYNC_ADMIN_TOKEN", "  ")])).unwrap();
        assert_eq!(cfg.admin_token, None);
        assert!(
            Config::from_lookup(lookup(&[("SMARAGD_SYNC_ALLOW_OPEN_REGISTRATION", "maybe")]))
                .is_err()
        );
        assert!(Config::from_lookup(lookup(&[("SMARAGD_SYNC_VAULT_QUOTA_MB", "lots")])).is_err());
    }
}
