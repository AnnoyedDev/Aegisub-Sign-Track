use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok().filter(|o| o.status.success())?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn main() {
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs");
    println!("cargo:rerun-if-env-changed=AEGISIGNTRACK_VERSION");
    let s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let z = s / 86400 + 719468;
    let (era, doe) = (z.div_euclid(146097), z.rem_euclid(146097));
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (d, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    let y = yoe + era * 400 + (m <= 2) as i64;
    let changed = git(&["status", "--porcelain"]).is_some_and(|s| !s.is_empty());
    let commit = match git(&["rev-parse", "--short=7", "HEAD"]) {
        Some(c) if changed => format!("{c}+local"),
        Some(c) => c,
        None => "local".into(),
    };
    let version = std::env::var("AEGISIGNTRACK_VERSION").unwrap_or_else(|_| format!("{y}.{m:02}.{d:02}-{commit}"));
    println!("cargo:rustc-env=AEGISIGNTRACK_BUILD={version}");
}
