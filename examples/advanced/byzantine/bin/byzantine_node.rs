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

//! Byzantine Generals Node - Multi-Process Consensus Demo
//!
//! This binary demonstrates PlexSpaces distributed actor system with Byzantine Generals consensus:
//! - Each node runs one or more generals as actors
//! - Nodes communicate via gRPC (ActorService)
//! - Votes are coordinated via distributed TupleSpace
//! - Supervision ensures fault tolerance
//!
//! ## Usage
//!
//! ### Multi-Process with Shared Redis (Recommended)
//!
//! ```bash
//! # Terminal 1: Start Redis
//! docker run -p 6379:6379 redis:7
//!
//! # Terminal 2: Node 1 with general0 (commander) and general1
//! cargo run --bin byzantine_node -- \
//!   --node-id node1 \
//!   --address localhost:9001 \
//!   --tuplespace-backend redis \
//!   --redis-url redis://localhost:6379 \
//!   --generals general0,general1
//!
//! # Terminal 3: Node 2 with general2 and general3
//! cargo run --bin byzantine_node -- \
//!   --node-id node2 \
//!   --address localhost:9002 \
//!   --tuplespace-backend redis \
//!   --redis-url redis://localhost:6379 \
//!   --generals general2,general3
//! ```
//!
//! ### Single-Process with In-Memory (Testing Only)
//!
//! ```bash
//! cargo run --bin byzantine_node -- \
//!   --node-id node1 \
//!   --address localhost:9001 \
//!   --tuplespace-backend memory \
//!   --generals general0,general1,general2,general3
//! ```
//!
//! ## Architecture
//!
//! ```text
//! Node 1 (localhost:9001)          Node 2 (localhost:9002)
//! ┌─────────────────────┐          ┌─────────────────────┐
//! │ General0 (Commander)│          │ General2            │
//! │ General1            │  ←gRPC→  │ General3            │
//! └─────────────────────┘          └─────────────────────┘
//!          ↓                                ↓
//!      ┌──────────────────────────────────────┐
//!      │   Distributed TupleSpace (votes)     │
//!      │   (Redis or in-memory shared)        │
//!      └──────────────────────────────────────┘
//! ```

use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;

use plexspaces_node::{Node, NodeId, NodeConfig};
use plexspaces_node::grpc_service::ActorServiceImpl;
use plexspaces_mailbox::{Mailbox, MailboxConfig};
use plexspaces::journal::MemoryJournal;
use plexspaces_core::ActorRef;

// TupleSpace imports
use plexspaces_tuplespace::TupleSpace;
// NOTE: StorageProvider and related types have been moved/renamed - using tuplespace crate directly

use byzantine_generals::{General, GeneralState, Decision};

use tonic::transport::Server;
use plexspaces_proto::ActorServiceServer;

/// Command-line arguments
#[derive(Debug)]
struct Args {
    node_id: String,
    address: String,
    generals: Vec<String>,
    /// TupleSpace backend: "memory", "redis", or "postgres"
    tuplespace_backend: String,
    /// Redis connection string (if backend=redis)
    redis_url: Option<String>,
    /// PostgreSQL connection string (if backend=postgres)
    postgres_url: Option<String>,
}

impl Args {
    fn parse() -> Result<Self, String> {
        let args: Vec<String> = std::env::args().collect();

        let mut node_id = None;
        let mut address = None;
        let mut generals = Vec::new();
        let mut tuplespace_backend = None;
        let mut redis_url = None;
        let mut postgres_url = None;

        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "--node-id" => {
                    i += 1;
                    if i < args.len() {
                        node_id = Some(args[i].clone());
                    }
                }
                "--address" => {
                    i += 1;
                    if i < args.len() {
                        address = Some(args[i].clone());
                    }
                }
                "--generals" => {
                    i += 1;
                    if i < args.len() {
                        generals = args[i].split(',').map(|s| s.trim().to_string()).collect();
                    }
                }
                "--tuplespace-backend" => {
                    i += 1;
                    if i < args.len() {
                        tuplespace_backend = Some(args[i].clone());
                    }
                }
                "--redis-url" => {
                    i += 1;
                    if i < args.len() {
                        redis_url = Some(args[i].clone());
                    }
                }
                "--postgres-url" => {
                    i += 1;
                    if i < args.len() {
                        postgres_url = Some(args[i].clone());
                    }
                }
                _ => {
                    return Err(format!("Unknown argument: {}", args[i]));
                }
            }
            i += 1;
        }

        Ok(Args {
            node_id: node_id.ok_or("Missing --node-id")?,
            address: address.ok_or("Missing --address")?,
            generals,
            tuplespace_backend: tuplespace_backend.unwrap_or_else(|| "memory".to_string()),
            redis_url,
            postgres_url,
        })
    }
}

/// Create TupleSpace with configured backend (memory, redis, or postgres)
async fn create_tuplespace(_args: &Args) -> Result<Arc<dyn byzantine_generals::TupleSpaceOps>, Box<dyn std::error::Error>> {
    // TODO: Update to use new TupleSpace API when storage API is finalized
    // For now, use in-memory TupleSpace
    println!("⚠️  Using in-memory TupleSpace - multi-process coordination will NOT work!");
    println!("   TODO: Update to use new TupleSpace storage API");
    
    let tuplespace = TupleSpace::new();
    
    // Cast to Arc<dyn TupleSpaceOps> for compatibility with General
    Ok(Arc::new(tuplespace) as Arc<dyn byzantine_generals::TupleSpaceOps>)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Parse arguments
    let args = Args::parse()?;

    println!("🚀 Starting Byzantine Node:");
    println!("  Node ID: {}", args.node_id);
    println!("  Address: {}", args.address);
    println!("  Generals: {:?}", args.generals);
    println!("  TupleSpace Backend: {}", args.tuplespace_backend);

    // Create PlexSpaces node
    let node_id = NodeId::new(&args.node_id);
    let config = NodeConfig {
        listen_addr: args.address.clone(),
        ..Default::default()
    };
    let node = Arc::new(Node::new(node_id, config));

    // Create shared TupleSpace for vote coordination
    let tuplespace = create_tuplespace(&args).await?;

    // Spawn generals on this node
    for (_i, general_id) in args.generals.iter().enumerate() {
        let is_commander = general_id == "general0";
        let actor_id = format!("{}@{}", general_id, args.node_id);

        // Create general state
        let state = GeneralState {
            id: general_id.clone(),
            is_commander,
            is_faulty: false,
            round: 0,
            votes: HashMap::new(),
            decision: Decision::Undecided,
            message_count: 0,
        };

        // Create mailbox
        let mailbox = Arc::new(Mailbox::new(MailboxConfig::default(), format!("mailbox-{}", actor_id.clone())).await?);

        // Create journal
        let journal = Arc::new(MemoryJournal::new());

        // Create general actor
        let general = General {
            id: actor_id.clone(),
            state: Arc::new(RwLock::new(state)),
            mailbox: mailbox.clone(),
            journal,
            tuplespace: tuplespace.clone(),
        };

        // Create ActorRef (pure data - just ID)
        let actor_ref = ActorRef::new(actor_id.clone())
            .map_err(|e| format!("Failed to create ActorRef: {}", e))?;

        // Register actor with node (requires ActorConfig as second parameter)
        node.register_actor(actor_ref, None).await?;

        println!("  ✅ Spawned {}: {}", if is_commander { "Commander" } else { "Lieutenant" }, general_id);
    }

    // Start gRPC server for remote actor communication
    let grpc_addr = args.address.parse()?;
    let grpc_service = ActorServiceImpl { node: node.clone() };

    println!("\n🌐 Starting gRPC server on {}", args.address);

    tokio::spawn(async move {
        Server::builder()
            .add_service(ActorServiceServer::new(grpc_service))
            .serve(grpc_addr)
            .await
            .expect("gRPC server failed");
    });

    println!("✅ Node ready! Press Ctrl+C to stop.\n");

    // Wait for shutdown signal
    tokio::signal::ctrl_c().await?;

    println!("\n👋 Shutting down...");
    Ok(())
}
