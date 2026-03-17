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

//! Service trait and service names for ServiceLocator

/// Trait for services that can be registered in ServiceLocator
pub trait Service: Send + Sync {
    /// Get the service name for registration
    fn service_name(&self) -> String;
}

/// Standard service names for consistent registration and lookup
pub mod service_names {
    /// Actor registry service
    pub const ACTOR_REGISTRY: &str = "ActorRegistry";
    /// Object registry service
    pub const OBJECT_REGISTRY: &str = "ObjectRegistry";
    /// Process group registry (local state)
    pub const PROCESS_GROUP_REGISTRY: &str = "ProcessGroupRegistry";
    /// Process group gRPC service (for remote access)
    pub const PROCESS_GROUP_SERVICE: &str = "ProcessGroupService";
    /// Reply waiter registry
    pub const REPLY_WAITER_REGISTRY: &str = "ReplyWaiterRegistry";
    /// Virtual actor manager
    pub const VIRTUAL_ACTOR_MANAGER: &str = "VirtualActorManager";
    /// Facet manager
    pub const FACET_MANAGER: &str = "FacetManager";
    /// Facet registry
    pub const FACET_REGISTRY: &str = "FacetRegistry";
    /// Actor factory implementation
    pub const ACTOR_FACTORY_IMPL: &str = "ActorFactoryImpl";
    /// Journal storage
    pub const JOURNAL_STORAGE: &str = "JournalStorage";
    /// Channel service
    pub const CHANNEL_SERVICE: &str = "ChannelService";
    /// Facet service
    pub const FACET_SERVICE: &str = "FacetService";
    /// Firecracker VM service
    pub const FIRECRACKER_VM_SERVICE: &str = "FirecrackerVmService";
    /// Node metrics accessor
    pub const NODE_METRICS_ACCESSOR: &str = "NodeMetricsAccessor";
    /// Node
    pub const NODE: &str = "Node";
    /// Tuplespace service
    pub const TUPLESPACE_SERVICE: &str = "TupleSpaceService";
}
