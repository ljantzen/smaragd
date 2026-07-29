//! Captures build-time metadata (git commit, working-tree cleanliness, build date)
//! as compile-time env vars, so `ui::about_panel` can show exactly which build is
//! running — useful for a dev tool with no auto-update/release channel of its own.
//! Shells out to `git` read-only (a metadata query, not a VCS operation) — safe
//! regardless of whether the repo is worked in day-to-day via git or jj, since jj
//! colocates with a real git store underneath.

use std::process::Command;

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(cmd).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|s| s.trim().to_string())
}

fn main() {
    let git_hash =
        run("git", &["rev-parse", "--short=8", "HEAD"]).unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=SMARAGD_GIT_HASH={git_hash}");

    let dirty = run("git", &["status", "--porcelain"]).is_some_and(|s| !s.is_empty());
    println!(
        "cargo:rustc-env=SMARAGD_GIT_DIRTY={}",
        if dirty { "-dirty" } else { "" }
    );

    let build_date =
        run("date", &["-u", "+%Y-%m-%d %H:%M UTC"]).unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=SMARAGD_BUILD_DATE={build_date}");

    // Re-run this script (and thus refresh the above) whenever HEAD moves or the
    // index changes, rather than only when smaragd's own source changes.
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");
}
