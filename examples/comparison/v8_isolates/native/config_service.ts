// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// V8 Isolates Implementation (Cloudflare Workers-like)
// Config Service - Control Plane

/**
 * Config Service using V8 Isolates (Cloudflare Workers pattern)
 * 
 * Features:
 * - Isolated execution contexts per worker
 * - Automatic scaling
 * - Edge distribution
 * - No shared state (stateless)
 */

export interface WorkerConfig {
  version: number;
  configJson: string;
  createdAt: Date;
  metadata: Record<string, string>;
}

export interface RegisterWorkerRequest {
  workerId: string;
  nodeId: string;
  currentVersion: number;
}

export interface RegisterWorkerResponse {
  latestVersion: number;
  config?: WorkerConfig;
}

export interface GetConfigRequest {
  workerId: string;
  version: number; // 0 for latest
}

export interface GetConfigResponse {
  config: WorkerConfig;
}

// In-memory config store (in production, use Durable Objects or KV)
const configStore = new Map<number, WorkerConfig>();
let latestVersion = 0;

// Worker registry (in production, use Durable Objects)
const workerRegistry = new Map<string, { nodeId: string; lastSeen: Date }>();

/**
 * Config Service Durable Object (Cloudflare Workers pattern)
 * Each worker group gets its own Durable Object instance
 */
export class ConfigServiceDO {
  private state: DurableObjectState;
  private env: Env;

  constructor(state: DurableObjectState, env: Env) {
    this.state = state;
    this.env = env;
  }

  async fetch(request: Request): Promise<Response> {
    const url = new URL(request.url);
    const path = url.pathname;

    if (path === "/register" && request.method === "POST") {
      return this.handleRegister(request);
    } else if (path === "/get-config" && request.method === "POST") {
      return this.handleGetConfig(request);
    } else if (path === "/notify-change" && request.method === "POST") {
      return this.handleNotifyChange(request);
    }

    return new Response("Not found", { status: 404 });
  }

  private async handleRegister(request: Request): Promise<Response> {
    const req: RegisterWorkerRequest = await request.json();
    
    // Update worker registry
    workerRegistry.set(req.workerId, {
      nodeId: req.nodeId,
      lastSeen: new Date(),
    });

    // Get latest config from storage
    const latestConfig = await this.state.storage.get<WorkerConfig>(
      `config:${latestVersion}`
    );

    const response: RegisterWorkerResponse = {
      latestVersion,
      config: latestConfig && latestConfig.version > req.currentVersion 
        ? latestConfig 
        : undefined,
    };

    return new Response(JSON.stringify(response), {
      headers: { "Content-Type": "application/json" },
    });
  }

  private async handleGetConfig(request: Request): Promise<Response> {
    const req: GetConfigRequest = await request.json();
    
    const version = req.version === 0 ? latestVersion : req.version;
    const config = await this.state.storage.get<WorkerConfig>(
      `config:${version}`
    );

    if (!config) {
      return new Response("Config not found", { status: 404 });
    }

    const response: GetConfigResponse = { config };
    return new Response(JSON.stringify(response), {
      headers: { "Content-Type": "application/json" },
    });
  }

  private async handleNotifyChange(request: Request): Promise<Response> {
    const { version, config } = await request.json();
    
    // Store new config
    await this.state.storage.put(`config:${version}`, config);
    latestVersion = version;

    // Notify workers (in production, use Durable Objects Alarms or Queue)
    // For demo, we'll use a simple broadcast mechanism
    const notified = await this.notifyWorkers(version);

    return new Response(JSON.stringify({ workersNotified: notified }), {
      headers: { "Content-Type": "application/json" },
    });
  }

  private async notifyWorkers(version: number): Promise<number> {
    // In production, use Durable Objects Alarms or Queue to notify workers
    // For demo, workers poll for updates
    return workerRegistry.size;
  }
}

/**
 * Worker implementation (runs in V8 isolate)
 */
export class Worker {
  private workerId: string;
  private nodeId: string;
  private currentConfigVersion: number = 0;
  private config: WorkerConfig | null = null;
  private configService: DurableObjectNamespace;

  constructor(
    workerId: string,
    nodeId: string,
    configService: DurableObjectNamespace
  ) {
    this.workerId = workerId;
    this.nodeId = nodeId;
    this.configService = configService;
  }

  async initialize(): Promise<void> {
    // Register and get initial config
    const id = this.configService.idFromName("config-service");
    const stub = this.configService.get(id);

    const response = await stub.fetch("https://config/register", {
      method: "POST",
      body: JSON.stringify({
        workerId: this.workerId,
        nodeId: this.nodeId,
        currentVersion: this.currentConfigVersion,
      }),
    });

    const result: RegisterWorkerResponse = await response.json();
    if (result.config) {
      this.config = result.config;
      this.currentConfigVersion = result.config.version;
      await this.applyConfig(result.config);
    }
  }

  async checkForUpdates(): Promise<void> {
    // Poll for config updates (in production, use WebSockets or Durable Objects Alarms)
    const id = this.configService.idFromName("config-service");
    const stub = this.configService.get(id);

    const response = await stub.fetch("https://config/get-config", {
      method: "POST",
      body: JSON.stringify({
        workerId: this.workerId,
        version: 0, // Get latest
      }),
    });

    if (response.ok) {
      const result: GetConfigResponse = await response.json();
      if (result.config.version > this.currentConfigVersion) {
        this.config = result.config;
        this.currentConfigVersion = result.config.version;
        await this.applyConfig(result.config);
      }
    }
  }

  private async applyConfig(config: WorkerConfig): Promise<void> {
    // Apply configuration locally
    const configData = JSON.parse(config.configJson);
    console.log(`Worker ${this.workerId} applying config version ${config.version}`);
    // Apply config to worker...
  }
}

/**
 * Main worker entry point (Cloudflare Workers pattern)
 */
export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
    
    // Route to Config Service Durable Object
    if (url.pathname.startsWith("/config")) {
      const id = env.CONFIG_SERVICE.idFromName("config-service");
      const stub = env.CONFIG_SERVICE.get(id);
      return stub.fetch(request);
    }

    return new Response("Not found", { status: 404 });
  },
};














