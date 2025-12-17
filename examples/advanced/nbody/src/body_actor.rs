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

//! Body Actor for N-Body Simulation

use async_trait::async_trait;
use plexspaces_core::{Actor, ActorContext, BehaviorError, BehaviorType};
use plexspaces_mailbox::Message;
use serde::{Deserialize, Serialize};

// Physics constant
const G: f64 = 6.67430e-11; // Gravitational constant (m^3 kg^-1 s^-2)

/// Represents a single body in the N-Body simulation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Body {
    pub id: String,
    pub mass: f64,        // kg
    pub position: [f64; 3], // [x, y, z] in meters
    pub velocity: [f64; 3], // [vx, vy, vz] in m/s
}

impl Body {
    pub fn new(id: String, mass: f64, position: [f64; 3], velocity: [f64; 3]) -> Self {
        Self {
            id,
            mass,
            position,
            velocity,
        }
    }

    /// Calculate distance to another body
    pub fn distance_to(&self, other: &Body) -> f64 {
        let dx = self.position[0] - other.position[0];
        let dy = self.position[1] - other.position[1];
        let dz = self.position[2] - other.position[2];
        (dx * dx + dy * dy + dz * dz).sqrt()
    }

    /// Calculate gravitational force from another body
    pub fn force_from(&self, other: &Body) -> [f64; 3] {
        let r = self.distance_to(other);
        if r < 1e-10 {
            // Avoid division by zero
            return [0.0, 0.0, 0.0];
        }

        let f_magnitude = G * self.mass * other.mass / (r * r);
        let dx = other.position[0] - self.position[0];
        let dy = other.position[1] - self.position[1];
        let dz = other.position[2] - self.position[2];

        let fx = f_magnitude * dx / r;
        let fy = f_magnitude * dy / r;
        let fz = f_magnitude * dz / r;

        [fx, fy, fz]
    }

    /// Update position and velocity based on force and time step
    pub fn update(&mut self, force: [f64; 3], dt: f64) {
        // F = ma, so a = F/m
        let ax = force[0] / self.mass;
        let ay = force[1] / self.mass;
        let az = force[2] / self.mass;

        // Update velocity: v = v0 + a*dt
        self.velocity[0] += ax * dt;
        self.velocity[1] += ay * dt;
        self.velocity[2] += az * dt;

        // Update position: x = x0 + v*dt
        self.position[0] += self.velocity[0] * dt;
        self.position[1] += self.velocity[1] * dt;
        self.position[2] += self.velocity[2] * dt;
    }
}

/// Body Actor Behavior
pub struct BodyActor {
    body: Body,
    /// List of other body IDs to interact with
    other_body_ids: Vec<String>,
    /// Accumulated force from all other bodies
    accumulated_force: [f64; 3],
}

impl BodyActor {
    pub fn new(body: Body) -> Self {
        Self {
            body,
            other_body_ids: Vec::new(),
            accumulated_force: [0.0, 0.0, 0.0],
        }
    }

    pub fn with_other_bodies(mut self, other_ids: Vec<String>) -> Self {
        self.other_body_ids = other_ids;
        self
    }

    /// Get current body state (for coordinator to read)
    pub fn get_state(&self) -> Body {
        self.body.clone()
    }

    /// Update body with force from another body
    pub fn add_force(&mut self, force: [f64; 3]) {
        self.accumulated_force[0] += force[0];
        self.accumulated_force[1] += force[1];
        self.accumulated_force[2] += force[2];
    }

    /// Apply accumulated force and update position/velocity
    pub fn apply_forces(&mut self, dt: f64) {
        self.body.update(self.accumulated_force, dt);
        // Reset accumulated force
        self.accumulated_force = [0.0, 0.0, 0.0];
    }
}

#[async_trait]
impl Actor for BodyActor {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let payload = String::from_utf8_lossy(&msg.payload);
        
        match payload.as_ref() {
            "get_state" => {
                // Return state to coordinator
                let body = self.get_state();
                let state_json = serde_json::to_string(&body)
                    .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;
                
                // Send state back to sender (coordinator)
                if let Some(sender_id) = msg.sender_id() {
                    let response = Message::new(state_json.into_bytes());
                    if let Some(actor_service) = ctx.get_actor_service().await {
                        actor_service
                            .send(sender_id, response)
                            .await
                            .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;
                    }
                }
            }
            _ => {
                // Check if this is a combined force+dt update
                if let Ok(update_json) = serde_json::from_slice::<serde_json::Value>(&msg.payload) {
                    // Handle combined update: {"force": [fx, fy, fz], "dt": 1.0}
                    if let Some(force_array) = update_json.get("force").and_then(|v| v.as_array()) {
                        if force_array.len() == 3 {
                            let force = [
                                force_array[0].as_f64().unwrap_or(0.0),
                                force_array[1].as_f64().unwrap_or(0.0),
                                force_array[2].as_f64().unwrap_or(0.0),
                            ];
                            self.add_force(force);
                        }
                    }
                    if let Some(dt) = update_json.get("dt").and_then(|v| v.as_f64()) {
                        // Apply accumulated forces and update position/velocity
                        self.apply_forces(dt);
                    }
                } else if let Ok(other_body) = serde_json::from_slice::<Body>(&msg.payload) {
                    // Calculate force from this other body
                    let body = self.get_state();
                    let force = body.force_from(&other_body);
                    self.add_force(force);
                }
            }
        }

        Ok(())
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

