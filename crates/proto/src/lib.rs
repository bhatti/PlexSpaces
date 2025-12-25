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

//! PlexSpaces Protocol Buffers
//!
//! Generated protobuf definitions for PlexSpaces

// Allow clippy warnings for generated code
#![allow(clippy::doc_lazy_continuation)]
#![allow(clippy::doc_overindented_list_items)]
#![allow(clippy::large_enum_variant)]

// Re-export prost_types for test usage
pub use prost_types;

// Include generated modules - these match the buf generated file names
pub mod common {
    pub mod v1 {
        include!("generated/plexspaces.common.v1.rs");
    }
}

pub mod actor {
    pub mod v1 {
        // Note: The actor.rs file already includes actor.tonic.rs at the end
        include!("generated/plexspaces.actor.v1.rs");
    }
}

pub mod behaviors {
    pub mod v1 {
        include!("generated/plexspaces.behaviors.v1.rs");
    }
}

pub mod facets {
    pub mod v1 {
        include!("generated/plexspaces.facets.v1.rs");
    }
}

pub mod tuplespace {
    pub mod v1 {
        include!("generated/plexspaces.tuplespace.v1.rs");
    }
}

// NOTE: tuplespace_registry and registry modules removed - use object_registry instead
// See: plexspaces_proto::object_registry::v1

pub mod processgroups {
    pub mod v1 {
        include!("generated/plexspaces.processgroups.v1.rs");
    }
}

pub mod node {
    pub mod v1 {
        include!("generated/plexspaces.node.v1.rs");
    }
}

pub mod supervision {
    pub mod v1 {
        include!("generated/plexspaces.supervision.v1.rs");
    }
}

pub mod grpc {
    pub mod v1 {
        include!("generated/plexspaces.grpc.v1.rs");
    }
}

pub mod metrics {
    pub mod v1 {
        include!("generated/plexspaces.metrics.v1.rs");
    }
}

pub mod system {
    pub mod v1 {
        include!("generated/plexspaces.system.v1.rs");
    }
}

pub mod storage {
    pub mod v1 {
        include!("generated/plexspaces.storage.v1.rs");
    }
}

pub mod journaling {
    pub mod v1 {
        include!("generated/plexspaces.journaling.v1.rs");
    }
}

pub mod timer {
    pub mod v1 {
        include!("generated/plexspaces.timer.v1.rs");
    }
}

pub mod application {
    pub mod v1 {
        include!("generated/plexspaces.application.v1.rs");
    }
}

pub mod channel {
    pub mod v1 {
        include!("generated/plexspaces.channel.v1.rs");
    }
}

pub mod dashboard {
    pub mod v1 {
        include!("generated/plexspaces.dashboard.v1.rs");
    }
}

pub mod pool {
    pub mod v1 {
        include!("generated/plexspaces.pool.v1.rs");
    }
}

pub mod workflow {
    pub mod v1 {
        include!("generated/plexspaces.workflow.v1.rs");
    }
}

pub mod mailbox {
    pub mod v1 {
        include!("generated/plexspaces.mailbox.v1.rs");
    }
}

pub mod wasm {
    pub mod v1 {
        include!("generated/plexspaces.wasm.v1.rs");
        // Note: tonic module is included at the end of wasm.v1.rs (similar to actor.v1.rs)
    }
}

pub mod firecracker {
    pub mod v1 {
        include!("generated/plexspaces.firecracker.v1.rs");
    }
}

pub mod circuitbreaker {
    pub mod prv {
        include!("generated/plexspaces.circuitbreaker.prv.rs");
    }
}

pub mod scheduling {
    pub mod v1 {
        include!("generated/plexspaces.scheduling.v1.rs");
    }
}

pub mod locks {
    pub mod prv {
        include!("generated/plexspaces.locks.prv.rs");
    }
}

pub mod security {
    pub mod v1 {
        include!("generated/plexspaces.security.v1.rs");
    }
}

// service_registry module removed - replaced by object_registry
// pub mod service_registry {
//     pub mod v1 {
//         include!("generated/plexspaces.service_registry.v1.rs");
//     }
// }

pub mod object_registry {
    pub mod v1 {
        include!("generated/plexspaces.object_registry.v1.rs");
    }
}

// Create v1 module with re-exports for convenience
pub mod v1 {
    // Note: plexspaces.v1.rs is not generated - no root proto file with that package
    // Re-export sub-modules
    pub use super::actor::v1 as actor;
    pub use super::application::v1 as application;
    pub use super::channel::v1 as channel;
    pub use super::circuitbreaker::prv as circuitbreaker;
    pub use super::common::v1 as common;
    pub use super::facets::v1 as facets;
    pub use super::grpc::v1 as grpc;
    pub use super::journaling::v1 as journaling;
    pub use super::timer::v1 as timer;
    pub use super::node::v1 as node;
    pub use super::pool::v1 as pool;
    pub use super::processgroups::v1 as processgroups;
    pub use super::workflow::v1 as workflow;
    pub use super::mailbox::v1 as mailbox;
    pub use super::wasm::v1 as wasm;
    pub use super::security::v1 as security;
    // service_registry removed - use object_registry instead
    // pub use super::service_registry::v1 as service_registry;
    pub use super::supervision::v1 as supervision;
    pub use super::tuplespace::v1 as tuplespace;
    pub use super::scheduling::v1 as scheduling;
    pub use super::storage::v1 as storage;
}

// Re-export v1 actor service for convenience
pub use v1::actor::{
    actor_lifecycle_event,
    actor_service_client::ActorServiceClient,
    actor_service_server::ActorService,
    actor_service_server::ActorServiceServer,
    ActorActivated,
    ActorCreated,
    ActorDeactivated,
    ActorDeactivating,
    ActorDownNotification,
    ActorFailed,
    // Lifecycle events
    ActorLifecycleEvent,
    ActorMigrating,
    ActorStarting,
    ActorTerminated,
    MonitorActorRequest,
    MonitorActorResponse,
};

// Re-export v1 tuplespace service for convenience
pub use v1::tuplespace::{
    tuple_plex_space_service_client::TuplePlexSpaceServiceClient,
    tuple_plex_space_service_server::TuplePlexSpaceService,
    tuple_plex_space_service_server::TuplePlexSpaceServiceServer,
};

// Re-export v1 process groups types for convenience
pub use v1::processgroups::{
    GroupMembership, ProcessGroup,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proto_modules_exist() {
        // Simple test to ensure modules are accessible
        // The actual types are tested in integration tests
        use v1::actor::Message;
        use v1::common::*;

        // Verify basic types exist
        let _ = Message::default();
    }
}
