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
pub use plexspaces_behavior as behavior; // OTP-style behaviors
pub use plexspaces_core as core; // Core types (ActorId, ActorContext, etc.)
pub use plexspaces_facet as facet; // Dynamic behavior composition
pub use plexspaces_keyvalue as keyvalue;
pub use plexspaces_lattice as lattice;
pub use plexspaces_mailbox as mailbox;
pub use plexspaces_node as node; // Distribution and clustering
pub use plexspaces_persistence as journal; // Pillar 3: Durable execution
pub use plexspaces_supervisor as supervision; // Pillar 2: Fault tolerance
pub use plexspaces_tuplespace as tuplespace; // Pillar 1: Universal coordination // Storage backend for registry and coordination

// Testing infrastructure for multi-environment support
pub mod testing;

// Simplified implementation (TDD approach) - currently broken, commented out
// pub mod simplified;

// Simple TDD module with minimal working implementation
pub mod simple_tdd;

// Release management (Erlang/OTP-inspired)
pub mod release;

// Application lifecycle management (Erlang/OTP-inspired)
pub mod application;

// PlexSpaces Node - top-level runtime (Erlang/OTP-inspired)
pub mod plexspaces_node;

// Re-export proto definitions from the proto crate (if available)
pub use plexspaces_proto as proto;

// Re-export core types for convenience
pub use actor::resource::{ActorHealth, ResourceContract, ResourceProfile, ResourceUsage};
pub use actor::{Actor as ActorStruct, ActorState};
pub use behavior::{GenServer, MessageType, MockBehavior};
pub use core::{
    Actor, ActorContext, ActorError, ActorId, ActorRef, BehaviorContext, BehaviorError,
    BehaviorType,
};
pub use journal::{Journal, JournalEntry, MemoryJournal};
pub use lattice::{
    ConsistencyLevel, CounterLattice, LWWLattice, Lattice, OrSetLattice, SetLattice, VectorClock,
};
pub use mailbox::{Mailbox, MailboxConfig, Message, MessagePriority, OrderingStrategy};
// TODO: Re-export node crate types when needed
// pub use node::{Node, NodeId, NodeConfig};
pub use supervision::{SupervisionStrategy, Supervisor};
// Re-export from our plexspaces_node module
pub use plexspaces_node::{NodeError, NodeState, PlexSpacesNode};
pub use tuplespace::{Pattern, Tuple, TupleSpace};

// Include tests module
#[cfg(test)]
mod tests;
