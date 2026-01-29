// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// TypeScript Actor Example for PlexSpaces
//
// This is a simple greeter actor that demonstrates TypeScript/WASM deployment
// to Firecracker VMs. In production, this would be compiled to WASM using Javy
// or similar tooling.

/**
 * Greeter Actor - Simple example actor in TypeScript
 */
export class GreeterActor {
    private name: string;

    constructor(name: string = "World") {
        this.name = name;
    }

    /**
     * Greet a person
     */
    greet(person?: string): string {
        const target = person || this.name;
        return `Hello, ${target}!`;
    }

    /**
     * Process a greeting request
     */
    processRequest(request: { name?: string; message?: string }): { response: string; timestamp: number } {
        const greeting = this.greet(request.name);
        return {
            response: greeting,
            timestamp: Date.now(),
        };
    }
}

// Example usage (for testing)
if (typeof require !== 'undefined' && require.main === module) {
    const greeter = new GreeterActor();
    console.log(greeter.greet("TypeScript"));
    console.log(greeter.processRequest({ name: "PlexSpaces" }));
}

