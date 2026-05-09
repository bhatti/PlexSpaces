// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 2.1 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Shared process-local resource sampling.
//!
//! Dashboard node metrics are expected to match the PlexSpaces process that operators inspect in
//! tools like Activity Monitor, so this sampler tracks the current process rather than host-wide
//! CPU and memory totals.

use sysinfo::{get_current_pid, Pid, ProcessRefreshKind, System};

/// Snapshot of the current PlexSpaces process resource usage.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProcessResourceSnapshot {
    /// Resident memory footprint in bytes.
    pub memory_used_bytes: u64,
    /// Process CPU usage percentage.
    pub cpu_usage_percent: f64,
}

/// Reusable sampler for process-local CPU and memory metrics.
#[derive(Debug)]
pub struct ProcessResourceSampler {
    system: System,
    pid: Pid,
}

impl ProcessResourceSampler {
    /// Create a sampler for the current process.
    pub fn new() -> Result<Self, &'static str> {
        let pid = get_current_pid()?;
        let mut system = System::new();
        system.refresh_process_specifics(pid, ProcessRefreshKind::new().with_memory().with_cpu());
        Ok(Self { system, pid })
    }

    /// Refresh and return the latest process-local resource usage.
    pub fn sample(&mut self) -> ProcessResourceSnapshot {
        self.system.refresh_cpu_usage();
        self.system.refresh_process_specifics(
            self.pid,
            ProcessRefreshKind::new().with_memory().with_cpu(),
        );

        self.system
            .process(self.pid)
            .map(|process| ProcessResourceSnapshot {
                memory_used_bytes: process.memory(),
                cpu_usage_percent: process.cpu_usage() as f64,
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::ProcessResourceSampler;

    #[test]
    fn sampler_reports_current_process_footprint() {
        let mut sampler = ProcessResourceSampler::new().expect("current pid should be available");
        let snapshot = sampler.sample();

        assert!(snapshot.memory_used_bytes > 0);
        assert!(snapshot.cpu_usage_percent >= 0.0);
    }
}
