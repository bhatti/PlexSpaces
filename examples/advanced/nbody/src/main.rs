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

//! N-Body Simulation Example
//!
//! Demonstrates parallel N-body gravitational simulation using PlexSpaces actors.
//! Each body is represented as an actor that calculates forces from other bodies.

mod body_actor;
mod config;

use body_actor::{Body, BodyActor};
use config::NBodyConfig;
use plexspaces_actor::ActorBuilder;
use plexspaces_mailbox::Message;
use plexspaces_node::{CoordinationComputeTracker, NodeBuilder};
use serde_json::json;
use std::f64::consts::PI;

/// Create 3-body system (Sun, Earth, Moon)
fn create_3body_system() -> Vec<Body> {
    vec![
        Body::new(
            "sun".to_string(),
            1.989e30, // Sun mass (kg)
            [0.0, 0.0, 0.0], // At origin
            [0.0, 0.0, 0.0], // At rest
        ),
        Body::new(
            "earth".to_string(),
            5.972e24, // Earth mass (kg)
            [1.496e11, 0.0, 0.0], // 1 AU from sun (x-axis)
            [0.0, 29.78e3, 0.0], // Orbital velocity (m/s)
        ),
        Body::new(
            "moon".to_string(),
            7.342e22, // Moon mass (kg)
            [1.496e11 + 3.844e8, 0.0, 0.0], // Near Earth
            [0.0, 29.78e3 + 1.022e3, 0.0], // Earth velocity + orbital
        ),
    ]
}

/// Generate bodies in circular orbits
fn generate_bodies(count: usize) -> Vec<Body> {
    let mut bodies = Vec::new();
    for i in 0..count {
        let angle = 2.0 * PI * (i as f64) / (count as f64);
        let radius = 1.0e11 * (1.0 + (i as f64) * 0.1); // Varying radii
        let mass = 1.0e24 * (1.0 + (i as f64) * 0.1); // Varying masses
        
        bodies.push(Body::new(
            format!("body-{}", i),
            mass,
            [radius * angle.cos(), radius * angle.sin(), 0.0],
            [-radius * angle.sin() * 1e3, radius * angle.cos() * 1e3, 0.0], // Orbital velocity
        ));
    }
    bodies
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .init();

    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║              N-Body Simulation Example                         ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();

    // Load configuration using ConfigBootstrap
    let config: NBodyConfig = NBodyConfig::load();
    config.validate()?;

    println!("Configuration:");
    println!("  Bodies: {}", config.body_count);
    println!("  Steps: {}", config.steps);
    println!("  Time step: {:.2} s", config.dt);
    println!();

    // Create bodies
    let bodies = if config.use_3body_system && config.body_count == 3 {
        create_3body_system()
    } else {
        generate_bodies(config.body_count)
    };

    // Create node using NodeBuilder
    let node = NodeBuilder::new("nbody-node")
        .build().await;

    // Create metrics tracker
    let mut metrics_tracker = CoordinationComputeTracker::new("nbody-simulation".to_string());

    // Spawn body actors
    println!("Spawning {} body actors...", bodies.len());
    metrics_tracker.start_coordinate();
    
    let body_ids: Vec<String> = bodies.iter().map(|b| b.id.clone()).collect();
    let mut body_refs = Vec::new();
    
    for (i, body) in bodies.iter().enumerate() {
        if i < 5 || i >= bodies.len() - 5 || bodies.len() <= 10 {
            println!("  ✅ Spawning: {}", body.id);
        } else if i == 5 {
            println!("  ... (spawning {} more actors) ...", bodies.len() - 10);
        }

        // Create list of other body IDs (all except current)
        let other_ids: Vec<String> = body_ids
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, id)| id.clone())
            .collect();

        // Create actor using ActorBuilder with larger mailbox capacity
        let actor = BodyActor::new(body.clone()).with_other_bodies(other_ids);
        let behavior = Box::new(actor);
        
        // Configure mailbox with larger capacity to avoid overflow
        use plexspaces_mailbox::MailboxConfig;
        let mut mailbox_config = MailboxConfig::default();
        mailbox_config.capacity = 10000; // Large capacity
        
        use plexspaces_actor::ActorBuilder;
        let ctx = plexspaces_core::RequestContext::internal();
        let actor_ref = ActorBuilder::new(behavior)
            .with_id(format!("{}@{}", body.id, node.id().as_str()))
            .with_mailbox_config(mailbox_config)
            .spawn(&ctx, node.service_locator().clone())
            .await
            .map_err(|e| format!("Failed to spawn actor: {}", e))?;
        
        body_refs.push((body.id.clone(), actor_ref));
    }
    
    metrics_tracker.end_coordinate();
    println!();

    // Wait for actors to initialize and start processing
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Get ActorService from node
    let actor_service = node.service_locator().get_actor_service().await
        .ok_or_else(|| "ActorService not available".to_string())?;

    // Simulation loop
    println!("Running simulation ({} steps)...", config.steps);
    println!();

    for step in 0..config.steps {
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("Step {}:", step);

        // Phase 1: Collect states from all bodies (coordination)
        metrics_tracker.start_coordinate();
        
        // Request state from all body actors (with delays to avoid mailbox overflow)
        for (idx, (_body_id, body_ref)) in body_refs.iter().enumerate() {
            let msg = Message::new(b"get_state".to_vec());
            actor_service.send(body_ref.id(), msg).await
                .map_err(|e| format!("Failed to send message: {}", e))?;
            // Delay between messages to allow processing
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }
        
        // Wait for responses
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        
        metrics_tracker.end_coordinate();

        // Phase 2: Calculate forces and update positions (computation)
        metrics_tracker.start_compute();
        
        // For each body, calculate forces from all other bodies
        // Note: In a full implementation, we'd collect states first, then broadcast
        // For simplicity, we calculate forces directly from initial body positions
        // Accumulate all forces for each body before sending
        for (i, (_body_id, body_ref)) in body_refs.iter().enumerate() {
            let mut total_force = [0.0, 0.0, 0.0];
            let body = &bodies[i];
            
            // Calculate forces from all other bodies
            for (j, other_body) in bodies.iter().enumerate() {
                if i != j {
                    let force = body.force_from(other_body);
                    total_force[0] += force[0];
                    total_force[1] += force[1];
                    total_force[2] += force[2];
                }
            }
            
            // Send accumulated force to body actor
            let force_json = json!({"force": [total_force[0], total_force[1], total_force[2]]});
            let msg = Message::new(serde_json::to_vec(&force_json)?);
            actor_service.send(body_ref.id(), msg).await
                .map_err(|e| format!("Failed to send message: {}", e))?;
            
            // Delay to allow processing and avoid mailbox overflow
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }
        
        // Apply forces and update positions
        let dt_json = json!({"dt": config.dt});
        for (_idx, (_body_id, body_ref)) in body_refs.iter().enumerate() {
            let msg = Message::new(serde_json::to_vec(&dt_json)?);
            actor_service.send(body_ref.id(), msg).await
                .map_err(|e| format!("Failed to send message: {}", e))?;
            // Delay to allow processing
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }
        
        // Wait for processing
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        
        metrics_tracker.end_compute();

        // Display current positions (simplified - just show step completed)
        println!("  Step {} completed", step);
        
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    // Finalize and display metrics
    let metrics = metrics_tracker.finalize();
    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("📊 Performance Metrics:");
    println!("  Coordination time: {:.2} ms", metrics.coordinate_duration_ms);
    println!("  Compute time: {:.2} ms", metrics.compute_duration_ms);
    println!("  Granularity ratio: {:.2}x", metrics.granularity_ratio);
    println!("  Efficiency: {:.2}%", metrics.efficiency * 100.0);
    println!();

    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║                    Simulation Complete                         ║");
    println!("╚════════════════════════════════════════════════════════════════╝");

    Ok(())
}
