//! Runtime toggles for verbose debug logging, set once at startup from CLI
//! flags (`--debug-sync`, `--debug-save`) and read from anywhere via
//! [`sync_enabled`]/[`save_enabled`]. Both default to off so normal runs stay
//! quiet; pass the flag when chasing a sync or save bug.
use std::sync::atomic::{AtomicBool, Ordering};

static SYNC: AtomicBool = AtomicBool::new(false);
static SAVE: AtomicBool = AtomicBool::new(false);

pub fn init(args: impl Iterator<Item = String>) {
    for arg in args {
        match arg.as_str() {
            "--debug-sync" => SYNC.store(true, Ordering::Relaxed),
            "--debug-save" => SAVE.store(true, Ordering::Relaxed),
            _ => {}
        }
    }
}

pub fn sync_enabled() -> bool {
    SYNC.load(Ordering::Relaxed)
}

pub fn save_enabled() -> bool {
    SAVE.load(Ordering::Relaxed)
}

/// `eprintln!`, but only when `--debug-sync` was passed on the command line.
#[macro_export]
macro_rules! sync_debug {
    ($($arg:tt)*) => {
        if $crate::debug_flags::sync_enabled() {
            eprintln!($($arg)*);
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_sets_only_the_matching_flag() {
        init(["--debug-sync".to_string()].into_iter());
        assert!(sync_enabled());
    }
}
