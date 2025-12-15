// SPDX-License-Identifier: LGPL-2.1-or-later
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

//! # TAP Device Networking
//!
//! ## Purpose
//! Provides TAP (network tap) device management for Firecracker VM networking.
//!
//! ## Architecture Context
//! TAP devices are virtual network interfaces that allow Firecracker microVMs
//! to communicate with the host and other VMs. This module manages the lifecycle
//! of TAP devices using Linux ioctl operations.
//!
//! ## Why This Exists
//! - Firecracker VMs need network connectivity
//! - TAP devices provide layer 2 (Ethernet) networking
//! - Enables VM-to-VM and VM-to-host communication
//! - Required for distributed actor systems across VMs
//!
//! ## Design Decisions
//! - TAP (not TUN) for Ethernet-level networking
//! - 10.0.0.0/24 IP range for VM network
//! - TAP names limited to 15 chars (Linux IFNAMSIZ)
//! - Generated from first 8 chars of VM ID: "tap-{vm_id[..8]}"
//!
//! ## Requirements
//! - Linux kernel with TUN/TAP support
//! - Root privileges or CAP_NET_ADMIN capability
//! - `/dev/net/tun` device must exist
//!
//! ## Network Architecture
//! ```text
//! ┌─────────────────────────────────────────────┐
//! │ Host (10.0.0.1)                             │
//! │                                             │
//! │  ┌──────────────────────────────────────┐  │
//! │  │ br-firecracker (bridge)              │  │
//! │  │ IP: 10.0.0.1/24                      │  │
//! │  └────┬──────────────┬──────────────────┘  │
//! │       │              │                      │
//! │   ┌───▼────┐     ┌───▼────┐                │
//! │   │tap-vm01│     │tap-vm02│                │
//! │   └───┬────┘     └───┬────┘                │
//! └───────┼──────────────┼─────────────────────┘
//!         │              │
//!    ┌────▼───┐     ┌────▼───┐
//!    │ VM #1  │     │ VM #2  │
//!    │10.0.0.2│     │10.0.0.3│
//!    └────────┘     └────────┘
//! ```
//!
//! ## Example
//! ```rust,no_run
//! use plexspaces_firecracker::network::{create_tap_device, delete_tap_device, generate_tap_name};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Generate TAP name from VM ID
//! let vm_id = "01HQXXXXXXXXXXXXXXXXXXXX"; // ULID
//! let tap_name = generate_tap_name(vm_id);
//! println!("TAP device: {}", tap_name); // "tap-01HQXXXX"
//!
//! // Create TAP device (requires root)
//! create_tap_device(&tap_name).await?;
//!
//! // ... use TAP device with Firecracker VM ...
//!
//! // Cleanup
//! delete_tap_device(&tap_name).await?;
//! # Ok(())
//! # }
//! ```

use crate::error::{FirecrackerError, FirecrackerResult};

/// Maximum length for Linux network interface names (IFNAMSIZ - 1)
const MAX_IFNAME_LEN: usize = 15;

/// Generate TAP device name from VM ID
///
/// ## Purpose
/// Creates a unique TAP device name from a VM ID following Linux naming constraints.
///
/// ## Arguments
/// * `vm_id` - VM identifier (typically ULID, 26 characters)
///
/// ## Returns
/// TAP device name in format "tap-{first8chars}"
///
/// ## Constraints
/// - Result is guaranteed to be <= 15 characters (Linux IFNAMSIZ limit)
/// - Uses first 8 characters of VM ID for uniqueness
/// - Prefix "tap-" is 4 chars, leaves 11 chars for ID (we use 8 for safety)
///
/// ## Example
/// ```rust
/// use plexspaces_firecracker::network::generate_tap_name;
///
/// let vm_id = "01HQXXXXXXXXXXXXXXXXXXXX"; // 26-char ULID
/// let tap_name = generate_tap_name(vm_id);
/// assert_eq!(tap_name.to_lowercase(), "tap-01hqxxxx");
/// assert!(tap_name.len() <= 15);
/// ```
///
/// ## Design Notes
/// - ULID first 8 chars provide ~1.2e14 combinations (collision-resistant)
/// - Shorter than VM ID but still unique for practical purposes
/// - Lowercase for consistency (some systems are case-insensitive)
pub fn generate_tap_name(vm_id: &str) -> String {
    // Take first 8 characters of VM ID (or less if shorter)
    let id_suffix = if vm_id.len() >= 8 {
        &vm_id[..8]
    } else {
        vm_id
    };

    // Format: "tap-{id}"
    // Total length: 4 (tap-) + 8 (id) = 12 chars (well under 15 limit)
    format!("tap-{}", id_suffix.to_lowercase())
}

/// Create TAP device for Firecracker VM networking
///
/// ## Purpose
/// Creates a virtual network interface (TAP device) on the host for VM networking.
///
/// ## Arguments
/// * `tap_name` - Name for the TAP device (max 15 chars)
///
/// ## Returns
/// `Ok(())` on success
///
/// ## Errors
/// - [`FirecrackerError::NetworkSetupFailed`]: Device creation failed
/// - [`FirecrackerError::ConfigurationError`]: Invalid tap_name (too long, invalid chars)
/// - [`FirecrackerError::IoError`]: Cannot access /dev/net/tun
///
/// ## Requirements
/// - **Root privileges** or CAP_NET_ADMIN capability
/// - `/dev/net/tun` device must exist
/// - Linux kernel with TUN/TAP support
///
/// ## Side Effects
/// - Creates TAP device visible in `ip link show`
/// - Device persists until explicitly deleted or system reboot
///
/// ## Example
/// ```rust,no_run
/// # use plexspaces_firecracker::network::create_tap_device;
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// // Requires root privileges
/// create_tap_device("tap-vm001").await?;
///
/// // Verify with: ip link show tap-vm001
/// # Ok(())
/// # }
/// ```
///
/// ## Implementation Notes
/// Uses Linux TUN/TAP driver via `/dev/net/tun` and ioctl `TUNSETIFF`.
/// See `man 4 tun` for details.
pub async fn create_tap_device(tap_name: &str) -> FirecrackerResult<()> {
    // Validate TAP name length
    if tap_name.len() > MAX_IFNAME_LEN {
        return Err(FirecrackerError::ConfigurationError(format!(
            "TAP name '{}' exceeds {} character limit",
            tap_name, MAX_IFNAME_LEN
        )));
    }

    #[cfg(target_os = "linux")]
    {
        if tap_name.is_empty() {
            return Err(FirecrackerError::ConfigurationError(
                "TAP name cannot be empty".to_string(),
            ));
        }

        // Use `ip tuntap add` command to create TAP device
        // This is simpler and more portable than using ioctl directly
        let output = tokio::process::Command::new("ip")
            .args(&["tuntap", "add", "mode", "tap", "name", tap_name])
            .output()
            .await
            .map_err(|e| {
                FirecrackerError::NetworkSetupFailed(format!(
                    "Failed to execute 'ip tuntap add': {}",
                    e
                ))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // If device already exists, that's OK (idempotent)
            if stderr.contains("File exists") || stderr.contains("exists") {
                return Ok(());
            }
            return Err(FirecrackerError::NetworkSetupFailed(format!(
                "Failed to create TAP device '{}': {}",
                tap_name, stderr
            )));
        }

        // Bring interface up
        let output = tokio::process::Command::new("ip")
            .args(&["link", "set", tap_name, "up"])
            .output()
            .await
            .map_err(|e| {
                FirecrackerError::NetworkSetupFailed(format!(
                    "Failed to bring TAP device up: {}",
                    e
                ))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Try to clean up the device we just created
            let _ = tokio::process::Command::new("ip")
                .args(&["tuntap", "del", "mode", "tap", "name", tap_name])
                .output()
                .await;
            return Err(FirecrackerError::NetworkSetupFailed(format!(
                "Failed to bring TAP device '{}' up: {}",
                tap_name, stderr
            )));
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        return Err(FirecrackerError::NetworkSetupFailed(
            "TAP devices are only supported on Linux".to_string(),
        ));
    }

    Ok(())
}

/// Delete TAP device
///
/// ## Purpose
/// Removes a TAP device from the system.
///
/// ## Arguments
/// * `tap_name` - Name of the TAP device to delete
///
/// ## Returns
/// `Ok(())` on success or if device doesn't exist
///
/// ## Errors
/// - [`FirecrackerError::NetworkSetupFailed`]: Deletion failed for existing device
///
/// ## Requirements
/// - **Root privileges** or CAP_NET_ADMIN capability (for existing devices)
///
/// ## Side Effects
/// - Removes TAP device from system
/// - Any packets in flight are dropped
/// - Firecracker VMs using this device will lose connectivity
///
/// ## Design Notes
/// - Safe to call on non-existent devices (returns Ok)
/// - Should be called in VM cleanup (Drop impl)
///
/// ## Example
/// ```rust,no_run
/// # use plexspaces_firecracker::network::delete_tap_device;
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// // Delete TAP device (requires root)
/// delete_tap_device("tap-vm001").await?;
///
/// // Safe to call multiple times
/// delete_tap_device("tap-vm001").await?; // Still Ok(())
/// # Ok(())
/// # }
/// ```
///
/// ## Implementation Notes
/// Uses `ip link delete <tap_name>` or equivalent ioctl.
pub async fn delete_tap_device(tap_name: &str) -> FirecrackerResult<()> {
    #[cfg(target_os = "linux")]
    {
        // Use `ip tuntap del` command to delete TAP device
        // Safe to call on non-existent devices (idempotent)
        let output = tokio::process::Command::new("ip")
            .args(&["tuntap", "del", "mode", "tap", "name", tap_name])
            .output()
            .await
            .map_err(|e| {
                FirecrackerError::NetworkSetupFailed(format!(
                    "Failed to execute 'ip tuntap del': {}",
                    e
                ))
            })?;

        // If device doesn't exist, that's OK (idempotent operation)
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Ignore "Cannot find device" errors (idempotent)
            if stderr.contains("Cannot find device")
                || stderr.contains("does not exist")
                || stderr.contains("No such device")
            {
                return Ok(());
            }
            // For other errors, return error (but don't fail if device already gone)
            return Err(FirecrackerError::NetworkSetupFailed(format!(
                "Failed to delete TAP device '{}': {}",
                tap_name, stderr
            )));
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = tap_name;
        return Err(FirecrackerError::NetworkSetupFailed(
            "TAP devices are only supported on Linux".to_string(),
        ));
    }

    Ok(())
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_tap_name() {
        // Standard ULID (26 chars)
        let vm_id = "01HQXXXXXXXXXXXXXXXXXXXX";
        let tap_name = generate_tap_name(vm_id);
        assert_eq!(tap_name, "tap-01hqxxxx");
        assert!(tap_name.len() <= MAX_IFNAME_LEN);
    }

    #[test]
    fn test_tap_name_length_limit() {
        // Very long VM ID
        let long_id = "a".repeat(100);
        let tap_name = generate_tap_name(&long_id);
        assert!(tap_name.len() <= MAX_IFNAME_LEN);
        assert_eq!(tap_name, "tap-aaaaaaaa");
    }

    #[test]
    fn test_tap_name_short_vm_id() {
        // Short VM ID (less than 8 chars)
        let short_id = "abc";
        let tap_name = generate_tap_name(short_id);
        assert_eq!(tap_name, "tap-abc");
        assert!(tap_name.len() <= MAX_IFNAME_LEN);
    }

    #[test]
    fn test_tap_name_empty_vm_id() {
        let tap_name = generate_tap_name("");
        assert_eq!(tap_name, "tap-");
        assert!(tap_name.len() <= MAX_IFNAME_LEN);
    }

    #[test]
    fn test_tap_name_lowercase() {
        let vm_id = "UPPERCASE123";
        let tap_name = generate_tap_name(vm_id);
        assert_eq!(tap_name, "tap-uppercas");
        assert!(tap_name.chars().all(|c| c.is_lowercase() || c == '-'));
    }

    #[tokio::test]
    async fn test_create_tap_valid_name() {
        // Valid name should succeed (even without root)
        let result = create_tap_device("tap-test001").await;
        // On Linux, this should return Ok (placeholder implementation)
        // On non-Linux, should error
        #[cfg(target_os = "linux")]
        assert!(result.is_ok());

        #[cfg(not(target_os = "linux"))]
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_tap_name_too_long() {
        // Name > 15 chars should error
        let long_name = "tap-verylongname123456789";
        let result = create_tap_device(long_name).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            FirecrackerError::ConfigurationError(_)
        ));
    }

    #[tokio::test]
    async fn test_create_tap_empty_name() {
        let result = create_tap_device("").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_delete_tap_nonexistent() {
        // Should not error (safe to delete non-existent)
        let result = delete_tap_device("tap-nosuch").await;
        #[cfg(target_os = "linux")]
        assert!(result.is_ok());

        #[cfg(not(target_os = "linux"))]
        assert!(result.is_err());
    }
}
