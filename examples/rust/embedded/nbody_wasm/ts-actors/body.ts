// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// N-Body Simulation - TypeScript Body Actor
//
// This TypeScript actor implements a single body in the N-Body simulation.
// It will be compiled to WASM and deployed as an application-level module.

// Physics constant
const G = 6.67430e-11; // Gravitational constant (m^3 kg^-1 s^-2)

// Body state interface
interface Body {
    id: string;
    mass: number;        // kg
    position: [number, number, number]; // [x, y, z] in meters
    velocity: [number, number, number]; // [vx, vy, vz] in m/s
}

// Body Actor implementation
export class BodyActor {
    private body: Body;
    private accumulatedForce: [number, number, number] = [0.0, 0.0, 0.0];

    constructor(body: Body) {
        this.body = body;
    }

    // Calculate distance to another body
    private distanceTo(other: Body): number {
        const dx = this.body.position[0] - other.position[0];
        const dy = this.body.position[1] - other.position[1];
        const dz = this.body.position[2] - other.position[2];
        return Math.sqrt(dx * dx + dy * dy + dz * dz);
    }

    // Calculate gravitational force from another body
    private forceFrom(other: Body): [number, number, number] {
        const r = this.distanceTo(other);
        if (r < 1e-10) {
            // Avoid division by zero
            return [0.0, 0.0, 0.0];
        }

        const fMagnitude = G * this.body.mass * other.mass / (r * r);
        const dx = other.position[0] - this.body.position[0];
        const dy = other.position[1] - this.body.position[1];
        const dz = other.position[2] - this.body.position[2];

        const fx = fMagnitude * dx / r;
        const fy = fMagnitude * dy / r;
        const fz = fMagnitude * dz / r;

        return [fx, fy, fz];
    }

    // Update position and velocity based on force and time step
    private update(force: [number, number, number], dt: number): void {
        // F = ma, so a = F/m
        const ax = force[0] / this.body.mass;
        const ay = force[1] / this.body.mass;
        const az = force[2] / this.body.mass;

        // Update velocity: v = v0 + a*dt
        this.body.velocity[0] += ax * dt;
        this.body.velocity[1] += ay * dt;
        this.body.velocity[2] += az * dt;

        // Update position: x = x0 + v*dt
        this.body.position[0] += this.body.velocity[0] * dt;
        this.body.position[1] += this.body.velocity[1] * dt;
        this.body.position[2] += this.body.velocity[2] * dt;
    }

    // Get current body state
    getState(): Body {
        return { ...this.body };
    }

    // Add force from another body
    addForce(force: [number, number, number]): void {
        this.accumulatedForce[0] += force[0];
        this.accumulatedForce[1] += force[1];
        this.accumulatedForce[2] += force[2];
    }

    // Apply accumulated force and update position/velocity
    applyForces(dt: number): void {
        this.update(this.accumulatedForce, dt);
        // Reset accumulated force
        this.accumulatedForce = [0.0, 0.0, 0.0];
    }

    // Handle message from coordinator
    handleMessage(messageType: string, payload: any): any {
        switch (messageType) {
            case "get_state":
                return this.getState();
            
            case "add_force":
                if (payload && payload.force) {
                    this.addForce(payload.force);
                }
                return { success: true };
            
            case "apply_forces":
                const dt = payload?.dt || 1.0;
                this.applyForces(dt);
                return { success: true };
            
            default:
                return { error: `Unknown message type: ${messageType}` };
        }
    }
}

// Export for WASM compilation
export default BodyActor;

