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

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get the crate directory (where Cargo.toml is)
    let crate_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);

    // Output directory (relative to crate, not build dir)
    let out_dir = crate_dir.join("src/generated");

    // Check if generated files already exist
    // If they do, skip proto compilation (use committed files)
    // BUT always run post-processing to fix Copy trait issues
    let files_exist = out_dir.exists() && out_dir.read_dir()?.next().is_some();
    if files_exist {
        println!("cargo:warning=Proto files already generated, skipping compilation");
        println!("cargo:warning=To regenerate: run 'make proto-vendor && make proto-buf'");
        // Still run post-processing to fix any Copy trait issues
        println!("cargo:warning=Running post-processing on existing files...");
        let generated_files = find_rust_files(&out_dir)?;
        for file_path in generated_files {
            let mut content = fs::read_to_string(&file_path)?;
            let original_content = content.clone();
            
            // Remove Copy from all prost::Message derives
            content = remove_copy_derives(content);
            
            if content != original_content {
                fs::write(&file_path, content)?;
            }
        }
        return Ok(());
    }

    println!("cargo:warning=Generating proto files from source...");

    // Check if PROTOC is set
    if env::var("PROTOC").is_err() {
        println!("cargo:warning=PROTOC environment variable not set!");
        println!("cargo:warning=To regenerate proto files, install protoc:");
        println!("cargo:warning=  https://grpc.io/docs/protoc-installation/");
        println!("cargo:warning=For now, using committed proto files.");
        return Ok(());
    }

    // Get workspace root (two levels up from crates/proto/)
    let workspace_root = crate_dir.parent().unwrap().parent().unwrap();

    // Proto directory is at workspace_root/proto
    let proto_dir = workspace_root.join("proto");
    let vendor_dir = proto_dir.join("vendor");

    // Create output directory if it doesn't exist
    fs::create_dir_all(&out_dir)?;

    // Find all .proto files in the main proto directory (excluding vendor)
    let mut proto_files = Vec::new();
    find_proto_files(&proto_dir, &mut proto_files, &vendor_dir)?;

    // Sort for deterministic builds
    proto_files.sort();

    println!("Found {} proto files to compile", proto_files.len());

    // Include directories for compilation
    let include_dirs = vec![proto_dir.clone(), vendor_dir.clone()];

    // Configure prost
    let mut config = prost_build::Config::new();

    // Compile well-known types from vendored dependencies
    config.compile_well_known_types();

    // Map google.protobuf to prost_types
    config.extern_path(".google.protobuf", "::prost_types");

    // Build with tonic
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .out_dir(&out_dir)
        .file_descriptor_set_path(out_dir.join("descriptor.bin"))
        .emit_rerun_if_changed(true)
        .compile_with_config(config, &proto_files, &include_dirs)?;

    // Post-process generated files to remove incorrect `Copy` derives
    println!("Post-processing generated files to remove incorrect `Copy` derives...");
    let generated_files = find_rust_files(&out_dir)?;
    for file_path in generated_files {
        let mut content = fs::read_to_string(&file_path)?;
        let original_content = content.clone();

        // First, remove Copy from ALL prost::Message derives (we'll be more selective if needed)
        // This is safer - prost types with Option<Timestamp> or Option<Duration> can't be Copy
        content = content.replace(
            "#[derive(Clone, Copy, PartialEq, ::prost::Message)]",
            "#[derive(Clone, PartialEq, ::prost::Message)]",
        );
        content = content.replace(
            "#[derive(Clone, Copy, PartialEq, Eq, ::prost::Message)]",
            "#[derive(Clone, PartialEq, Eq, ::prost::Message)]",
        );
        // Remove `Copy` from `prost::Oneof` derives
        content = content.replace(
            "#[derive(Clone, Copy, PartialEq, ::prost::Oneof)]",
            "#[derive(Clone, PartialEq, ::prost::Oneof)]",
        );
        content = content.replace(
            "#[derive(Clone, Copy, PartialEq, Eq, ::prost::Oneof)]",
            "#[derive(Clone, PartialEq, Eq, ::prost::Oneof)]",
        );
        // Remove `Copy` from enum derives (prost::Enumeration) - enums can be Copy if all variants are Copy
        // But we're being conservative and removing Copy from all for consistency
        content = content.replace(
            "#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]",
            "#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]",
        );
        content = content.replace(
            "#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]",
            "#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]",
        );
        // Remove standalone Copy derives (more patterns)
        content = content.replace(", Copy,", ",");
        content = content.replace(", Copy)", ")");
        content = content.replace("(Copy,", "(");
        content = content.replace("(Copy)", "()");
        // Replace prost's Empty with unit type for better ergonomics
        content = content.replace("::prost_types::Empty", "()");

        if content != original_content {
            fs::write(&file_path, content)?;
        }
    }
    println!("Post-processing complete!");

    // Tell cargo to rerun if proto files change
    println!("cargo:rerun-if-changed={}", proto_dir.display());

    println!(
        "Proto generation complete! Generated {} proto files",
        proto_files.len()
    );

    Ok(())
}

/// Remove Copy derives from generated proto code
fn remove_copy_derives(content: String) -> String {
    let mut content = content;
    
    // Remove `Copy` from `prost::Message` derives (multiple patterns)
    // Pattern: #[derive(Clone, Copy, PartialEq, ::prost::Message)]
    content = content.replace(
        "#[derive(Clone, Copy, PartialEq, ::prost::Message)]",
        "#[derive(Clone, PartialEq, ::prost::Message)]",
    );
    // Pattern: #[derive(Clone, Copy, PartialEq, Eq, ::prost::Message)]
    content = content.replace(
        "#[derive(Clone, Copy, PartialEq, Eq, ::prost::Message)]",
        "#[derive(Clone, PartialEq, Eq, ::prost::Message)]",
    );
    // Pattern: #[derive(Clone, Copy, PartialEq, ::prost::Message)] (with different spacing)
    content = content.replace(
        "#[derive(Clone,Copy,PartialEq,::prost::Message)]",
        "#[derive(Clone,PartialEq,::prost::Message)]",
    );
    // Remove `Copy` from `prost::Oneof` derives
    content = content.replace(
        "#[derive(Clone, Copy, PartialEq, ::prost::Oneof)]",
        "#[derive(Clone, PartialEq, ::prost::Oneof)]",
    );
    content = content.replace(
        "#[derive(Clone, Copy, PartialEq, Eq, ::prost::Oneof)]",
        "#[derive(Clone, PartialEq, Eq, ::prost::Oneof)]",
    );
    // Remove `Copy` from enum derives (prost::Enumeration)
    content = content.replace(
        "#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]",
        "#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]",
    );
    content = content.replace(
        "#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]",
        "#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]",
    );
    // Remove standalone Copy derives (more patterns)
    content = content.replace(", Copy,", ",");
    content = content.replace(", Copy)", ")");
    content = content.replace("(Copy,", "(");
    content = content.replace("(Copy)", "()");
    // Replace prost's Empty with unit type for better ergonomics
    content = content.replace("::prost_types::Empty", "()");
    
    content
}

/// Recursively find all .proto files in a directory, excluding a path.
fn find_proto_files(dir: &Path, files: &mut Vec<PathBuf>, exclude: &Path) -> std::io::Result<()> {
    if dir.is_dir() {
        // Skip the vendor directory
        if dir.canonicalize()? == exclude.canonicalize()? {
            return Ok(());
        }

        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                find_proto_files(&path, files, exclude)?;
            } else if path.extension().and_then(|s| s.to_str()) == Some("proto") {
                files.push(path);
            }
        }
    }
    Ok(())
}

/// Recursively find all .rs files in a directory.
fn find_rust_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                files.extend(find_rust_files(&path)?);
            } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                files.push(path);
            }
        }
    }
    Ok(files)
}
