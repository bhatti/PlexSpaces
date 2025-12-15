#!/bin/bash

set -euo pipefail

GEN_DIR="${1:-src/generated}"

echo "Generating mod.rs files in $GEN_DIR"

# Create the main mod.rs for generated code
cat > "$GEN_DIR/mod.rs" << 'EOF'
//! Generated protobuf and gRPC code for plexspaces framework
//!
//! This module contains all generated code from protocol buffer definitions.
//! Code is automatically generated - do not edit manually.

#![allow(clippy::all)]
#![allow(dead_code)]
#![allow(unused_imports)]

pub mod plexspaces {
    pub mod common {
        pub mod v1 {
            include!("plexspaces.common.v1.rs");
        }
    }

    pub mod actor {
        pub mod v1 {
            include!("plexspaces.actor.v1.rs");
        }
    }

    pub mod persistence {
        pub mod v1 {
            include!("plexspaces.persistence.v1.rs");
        }
    }

    pub mod microvm {
        pub mod v1 {
            include!("plexspaces.microvm.v1.rs");
        }
    }

    pub mod workflow {
        pub mod v1 {
            include!("plexspaces.workflow.v1.rs");
        }
    }

    pub mod tuplespace {
        pub mod v1 {
            include!("plexspaces.tuplespace.v1.rs");
        }
    }

    pub mod mobility {
        pub mod v1 {
            include!("plexspaces.mobility.v1.rs");
        }
    }

    pub mod system {
        pub mod v1 {
            include!("plexspaces.system.v1.rs");
        }
    }

    pub mod security {
        pub mod v1 {
            include!("plexspaces.security.v1.rs");
        }
    }
}

// Re-export commonly used types
pub use plexspaces::common::v1::*;
EOF

# Make sure the directory exists
mkdir -p "$GEN_DIR"

echo "✅ Generated mod.rs files successfully"


