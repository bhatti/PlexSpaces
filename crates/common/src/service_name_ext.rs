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

//! Extension trait for `ServiceName` proto enum — maps to stable registration key strings.

use plexspaces_proto::services::prv::ServiceName;

/// Maps a `ServiceName` proto enum variant to the stable `&str` key used for
/// ServiceLocator registration and lookup.
pub trait ServiceNameExt {
    /// Returns the stable registration key string for this service name.
    fn as_str(&self) -> &'static str;
}

impl ServiceNameExt for ServiceName {
    fn as_str(&self) -> &'static str {
        match self {
            ServiceName::ServiceNameUnspecified => "Unspecified",
            ServiceName::ServiceNameActorRegistry => "ActorRegistry",
            ServiceName::ServiceNameObjectRegistry => "ObjectRegistry",
            ServiceName::ServiceNameProcessGroupRegistry => "ProcessGroupRegistry",
            ServiceName::ServiceNameProcessGroupService => "ProcessGroupService",
            ServiceName::ServiceNameReplyWaiterRegistry => "ReplyWaiterRegistry",
            ServiceName::ServiceNameVirtualActorManager => "VirtualActorManager",
            ServiceName::ServiceNameFacetManager => "FacetManager",
            ServiceName::ServiceNameFacetRegistry => "FacetRegistry",
            ServiceName::ServiceNameActorFactoryImpl => "ActorFactoryImpl",
            ServiceName::ServiceNameJournalStorage => "JournalStorage",
            ServiceName::ServiceNameChannelService => "ChannelService",
            ServiceName::ServiceNameFacetService => "FacetService",
            ServiceName::ServiceNameFirecrackerVmService => "FirecrackerVmService",
            ServiceName::ServiceNameNode => "Node",
            ServiceName::ServiceNameTuplespaceService => "TupleSpaceService",
            ServiceName::ServiceNameApplicationService => "ApplicationService",
            ServiceName::ServiceNameActorService => "ActorService",
            ServiceName::ServiceNameBlobService => "BlobService",
            ServiceName::ServiceNameWorkflowService => "WorkflowService",
            ServiceName::ServiceNameSystemService => "SystemService",
            ServiceName::ServiceNameNodeService => "NodeService",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_name_as_str() {
        assert_eq!(
            ServiceName::ServiceNameActorRegistry.as_str(),
            "ActorRegistry"
        );
        assert_eq!(
            ServiceName::ServiceNameObjectRegistry.as_str(),
            "ObjectRegistry"
        );
        assert_eq!(
            ServiceName::ServiceNameProcessGroupRegistry.as_str(),
            "ProcessGroupRegistry"
        );
        assert_eq!(
            ServiceName::ServiceNameProcessGroupService.as_str(),
            "ProcessGroupService"
        );
        assert_eq!(
            ServiceName::ServiceNameReplyWaiterRegistry.as_str(),
            "ReplyWaiterRegistry"
        );
        assert_eq!(
            ServiceName::ServiceNameVirtualActorManager.as_str(),
            "VirtualActorManager"
        );
        assert_eq!(
            ServiceName::ServiceNameFacetManager.as_str(),
            "FacetManager"
        );
        assert_eq!(
            ServiceName::ServiceNameFacetRegistry.as_str(),
            "FacetRegistry"
        );
        assert_eq!(
            ServiceName::ServiceNameActorFactoryImpl.as_str(),
            "ActorFactoryImpl"
        );
        assert_eq!(
            ServiceName::ServiceNameJournalStorage.as_str(),
            "JournalStorage"
        );
        assert_eq!(
            ServiceName::ServiceNameChannelService.as_str(),
            "ChannelService"
        );
        assert_eq!(
            ServiceName::ServiceNameFacetService.as_str(),
            "FacetService"
        );
        assert_eq!(
            ServiceName::ServiceNameFirecrackerVmService.as_str(),
            "FirecrackerVmService"
        );
        assert_eq!(ServiceName::ServiceNameNode.as_str(), "Node");
        assert_eq!(
            ServiceName::ServiceNameTuplespaceService.as_str(),
            "TupleSpaceService"
        );
    }
}
