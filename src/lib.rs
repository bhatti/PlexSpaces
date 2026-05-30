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

//! PlexSpaces: A unified distributed actor framework
//!
//! Core design philosophy:
//! - One powerful abstraction is worth ten specialized features
//! - Elevate concepts from research into generalized abstractions
//! - Composable capabilities over specialized types
//!
//! Five Foundational Pillars:
//! 1. TupleSpace coordination (from previous project)
//! 2. Erlang/OTP supervision and fault tolerance
//! 3. Durable execution with journaling (Restate-inspired)
//! 4. WASM runtime for portable actors
//! 5. Firecracker VMs for isolation

#![warn(missing_docs)]
#![warn(rustdoc::missing_crate_level_docs)]

// Core modules that implement the 5 pillars
// Independent crates - re-export them here
pub use plexspaces_actor as actor; // Pillar 2: Erlang/OTP actors
pub use plexspaces_actor as core; // Core types (ActorId, ActorContext, etc.) — merged into actor
pub use plexspaces_actor::supervisor as supervision; // Pillar 2: Fault tolerance (merged into actor crate)
pub use plexspaces_actor::behavior; // OTP-style behaviors
pub use plexspaces_facet as facet; // Dynamic behavior composition
pub use plexspaces_keyvalue as keyvalue;
pub use plexspaces_lattice as lattice;
pub use plexspaces_mailbox as mailbox;
pub use plexspaces_node as node; // Distribution and clustering
pub use plexspaces_persistence as journal; // Pillar 3: Durable execution
pub use plexspaces_tuplespace as tuplespace; // Pillar 1: Universal coordination // Storage backend for registry and coordination

// Re-export release parser from common crate
pub use plexspaces_common::release_parser as release;

// Re-export proto definitions from the proto crate (if available)
pub use plexspaces_proto as proto;

// Re-export core types for convenience
pub use actor::resource::{ActorHealth, ResourceContract, ResourceProfile, ResourceUsage};
pub use actor::{ActorInstance as ActorStruct, ActorState};
pub use behavior::{GenServer, MessageType, MockBehavior};
pub use core::actor_types::Actor;
pub use core::{
    ActorContext, ActorError, ActorId, ActorRef, BehaviorContext, BehaviorError, BehaviorType,
};
pub use journal::{Journal, JournalEntry, MemoryJournal};
pub use lattice::{
    ConsistencyLevel, CounterLattice, LWWLattice, Lattice, OrSetLattice, SetLattice, VectorClock,
};
pub use mailbox::{Mailbox, MailboxConfig, MessagePriority, OrderingStrategy};
pub use plexspaces_proto::common::v1::Message;
// TODO: Re-export node crate types when needed
// pub use node::{Node, NodeId, NodeConfig};
pub use supervision::{SupervisionStrategy, Supervisor};
pub use tuplespace::{Pattern, Tuple, TupleSpace};
