use std::process::Command;

fn main() {
    println!(
        "cargo:rustc-env=PLEXSPACES_BUILD_DATE={}",
        chrono::Utc::now().to_rfc3339()
    );

    let git_commit = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=PLEXSPACES_GIT_COMMIT={git_commit}");
}
