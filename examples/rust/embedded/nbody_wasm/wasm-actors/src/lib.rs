// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Simple Rust WASM actor for testing when TypeScript/Javy is not available

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Body {
    pub id: String,
    pub mass: f64,
    pub position: [f64; 3],
    pub velocity: [f64; 3],
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BodyActor {
    body: Body,
    accumulated_force: [f64; 3],
}

impl BodyActor {
    pub fn new(body: Body) -> Self {
        Self {
            body,
            accumulated_force: [0.0, 0.0, 0.0],
        }
    }

    pub fn get_state(&self) -> &Body {
        &self.body
    }

    pub fn add_force(&mut self, force: [f64; 3]) {
        self.accumulated_force[0] += force[0];
        self.accumulated_force[1] += force[1];
        self.accumulated_force[2] += force[2];
    }

    pub fn apply_forces(&mut self, dt: f64) {
        // F = ma, so a = F/m
        let ax = self.accumulated_force[0] / self.body.mass;
        let ay = self.accumulated_force[1] / self.body.mass;
        let az = self.accumulated_force[2] / self.body.mass;

        // Update velocity: v = v0 + a*dt
        self.body.velocity[0] += ax * dt;
        self.body.velocity[1] += ay * dt;
        self.body.velocity[2] += az * dt;

        // Update position: x = x0 + v*dt
        self.body.position[0] += self.body.velocity[0] * dt;
        self.body.position[1] += self.body.velocity[1] * dt;
        self.body.position[2] += self.body.velocity[2] * dt;

        // Reset accumulated force
        self.accumulated_force = [0.0, 0.0, 0.0];
    }
}

// WASM entry points would go here
// For now, this is a placeholder that can be compiled to WASM

