// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Build script for plexspaces-node
// Extracts framework version and build information

fn main() {
    // Rerun if Cargo.toml changes
    println!("cargo:rerun-if-changed=Cargo.toml");
    
    // Extract version from Cargo.toml (via CARGO_PKG_VERSION)
    let version = env!("CARGO_PKG_VERSION");
    println!("cargo:rustc-env=PLEXSPACES_FRAMEWORK_VERSION={}", version);
    
    // Get build date (ISO 8601 format)
    // Use SOURCE_DATE_EPOCH if available (for reproducible builds), otherwise current time
    let build_date = if let Ok(epoch) = std::env::var("SOURCE_DATE_EPOCH") {
        if let Ok(epoch) = epoch.parse::<i64>() {
            format_timestamp(epoch)
        } else {
            format_timestamp(std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64)
        }
    } else {
        format_timestamp(std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64)
    };
    println!("cargo:rustc-env=PLEXSPACES_BUILD_DATE={}", build_date);
    
    // Get git commit hash if available
    let git_commit = std::process::Command::new("git")
        .args(&["rev-parse", "HEAD"])
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                String::from_utf8(output.stdout).ok()
            } else {
                None
            }
        })
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=PLEXSPACES_GIT_COMMIT={}", git_commit);
    
    // Get build target
    let build_target = std::env::var("TARGET").unwrap_or_else(|_| "unknown".to_string());
    println!("cargo:rustc-env=PLEXSPACES_BUILD_TARGET={}", build_target);
}

/// Format Unix timestamp as ISO 8601 string
fn format_timestamp(epoch_secs: i64) -> String {
    use std::time::{SystemTime, UNIX_EPOCH, Duration};
    
    let datetime = UNIX_EPOCH + Duration::from_secs(epoch_secs as u64);
    let system_time = SystemTime::from(datetime);
    
    // Format as ISO 8601 (simplified, UTC)
    // Format: YYYY-MM-DDTHH:MM:SSZ
    // This is a simplified formatter - for production, consider using a proper date library
    // or relying on runtime formatting with chrono
    let secs = epoch_secs;
    let days = secs / 86400;
    let secs_in_day = secs % 86400;
    
    // Calculate date (simplified, assumes 1970-01-01 as epoch)
    let year = 1970 + (days / 365);
    let day_of_year = days % 365;
    let month = (day_of_year / 30) + 1;
    let day = (day_of_year % 30) + 1;
    let hour = secs_in_day / 3600;
    let minute = (secs_in_day % 3600) / 60;
    let second = secs_in_day % 60;
    
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", year, month, day, hour, minute, second)
}

