// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// V8 Isolates Implementation (Cloudflare Workers-like)
// Data Pipeline - Data Plane

/**
 * Data Pipeline using V8 Isolates (Cloudflare Workers pattern)
 * 
 * Pipeline: Ingestion → Filter → Enrich → Transform → Destination
 * 
 * Features:
 * - Isolated execution per event
 * - JavaScript sandbox for user functions
 * - Backpressure handling
 * - Durability via Durable Objects
 */

export interface LogEntry {
  id: string;
  timestamp: Date;
  level: string;
  message: string;
  fields: Record<string, any>;
  source: string;
}

export interface MetricEntry {
  id: string;
  timestamp: Date;
  name: string;
  value: number;
  metricType: string;
  tags: Record<string, string>;
  source: string;
}

export type PipelineEvent = 
  | { type: "log"; data: LogEntry }
  | { type: "metric"; data: MetricEntry };

export interface PipelineConfig {
  pipelineId: string;
  filterFunction: string;
  enrichmentFunction: string;
  transformFunction: string;
  destinations: DestinationConfig[];
  backpressureThreshold: number;
  durabilityEnabled: boolean;
}

export interface DestinationConfig {
  destinationType: "splunk" | "datadog" | "kinesis" | "s3" | "elasticsearch";
  configJson: string;
  retryConfig: RetryConfig;
}

export interface RetryConfig {
  maxRetries: number;
  initialBackoffSec: number;
  maxBackoffSec: number;
  backoffMultiplier: number;
}

/**
 * Pipeline Processor Durable Object
 * Handles pipeline processing with durability
 */
export class PipelineProcessorDO {
  private state: DurableObjectState;
  private env: Env;
  private pendingEvents: PipelineEvent[] = [];
  private config: PipelineConfig | null = null;

  constructor(state: DurableObjectState, env: Env) {
    this.state = state;
    this.env = env;
  }

  async fetch(request: Request): Promise<Response> {
    const url = new URL(request.url);
    const path = url.pathname;

    if (path === "/process" && request.method === "POST") {
      return this.handleProcess(request);
    } else if (path === "/configure" && request.method === "POST") {
      return this.handleConfigure(request);
    }

    return new Response("Not found", { status: 404 });
  }

  private async handleProcess(request: Request): Promise<Response> {
    const events: PipelineEvent[] = await request.json();

    // Check backpressure
    const pendingCount = await this.state.storage.get<number>("pending_count") || 0;
    if (pendingCount >= (this.config?.backpressureThreshold || 1000)) {
      return new Response(
        JSON.stringify({ error: "Backpressure: too many pending events" }),
        { status: 429 }
      );
    }

    // Process events through pipeline
    const processed = await this.processPipeline(events);

    // Store pending events if durability enabled
    if (this.config?.durabilityEnabled) {
      await this.storePendingEvents(processed);
    }

    // Send to destinations
    const results = await Promise.allSettled(
      processed.map(event => this.sendToDestinations(event))
    );

    return new Response(JSON.stringify({ processed: processed.length }), {
      headers: { "Content-Type": "application/json" },
    });
  }

  private async handleConfigure(request: Request): Promise<Response> {
    const config: PipelineConfig = await request.json();
    this.config = config;
    await this.state.storage.put("config", config);
    return new Response(JSON.stringify({ success: true }), {
      headers: { "Content-Type": "application/json" },
    });
  }

  private async processPipeline(events: PipelineEvent[]): Promise<PipelineEvent[]> {
    let processed = events;

    // Filter stage
    if (this.config?.filterFunction) {
      processed = await this.applyFilter(processed);
    }

    // Enrichment stage
    if (this.config?.enrichmentFunction) {
      processed = await this.applyEnrichment(processed);
    }

    // Transformation stage
    if (this.config?.transformFunction) {
      processed = await this.applyTransform(processed);
    }

    return processed;
  }

  private async applyFilter(events: PipelineEvent[]): Promise<PipelineEvent[]> {
    // Execute filter function in sandbox
    const filterFn = this.compileFunction(this.config!.filterFunction);
    return events.filter(event => {
      try {
        return filterFn(event);
      } catch (e) {
        console.error("Filter error:", e);
        return false; // Drop event on error
      }
    });
  }

  private async applyEnrichment(events: PipelineEvent[]): Promise<PipelineEvent[]> {
    // Execute enrichment function in sandbox
    const enrichFn = this.compileFunction(this.config!.enrichmentFunction);
    return events.map(event => {
      try {
        const enriched = enrichFn(event);
        return enriched || event;
      } catch (e) {
        console.error("Enrichment error:", e);
        return event; // Return original on error
      }
    });
  }

  private async applyTransform(events: PipelineEvent[]): Promise<PipelineEvent[]> {
    // Execute transform function in sandbox
    const transformFn = this.compileFunction(this.config!.transformFunction);
    return events.map(event => {
      try {
        return transformFn(event);
      } catch (e) {
        console.error("Transform error:", e);
        return event; // Return original on error
      }
    });
  }

  private compileFunction(code: string): Function {
    // In production, use a proper sandbox (e.g., vm2, isolated-vm)
    // For demo, we'll use eval (NOT SAFE FOR PRODUCTION)
    // In Cloudflare Workers, use WebAssembly or restricted eval
    return new Function("event", code);
  }

  private async sendToDestinations(event: PipelineEvent): Promise<void> {
    if (!this.config?.destinations) return;

    for (const dest of this.config.destinations) {
      await this.sendToDestination(event, dest);
    }
  }

  private async sendToDestination(
    event: PipelineEvent,
    dest: DestinationConfig
  ): Promise<void> {
    const config = JSON.parse(dest.configJson);
    let retries = 0;
    let backoff = dest.retryConfig.initialBackoffSec;

    while (retries <= dest.retryConfig.maxRetries) {
      try {
        switch (dest.destinationType) {
          case "splunk":
            await this.sendToSplunk(event, config);
            return;
          case "datadog":
            await this.sendToDatadog(event, config);
            return;
          case "kinesis":
            await this.sendToKinesis(event, config);
            return;
          case "s3":
            await this.sendToS3(event, config);
            return;
          case "elasticsearch":
            await this.sendToElasticsearch(event, config);
            return;
        }
      } catch (e) {
        retries++;
        if (retries > dest.retryConfig.maxRetries) {
          // Store in DLQ or log error
          console.error(`Failed to send to ${dest.destinationType}:`, e);
          if (this.config?.durabilityEnabled) {
            await this.storeFailedEvent(event, dest);
          }
          throw e;
        }
        // Exponential backoff
        await new Promise(resolve => setTimeout(resolve, backoff * 1000));
        backoff = Math.min(
          backoff * dest.retryConfig.backoffMultiplier,
          dest.retryConfig.maxBackoffSec
        );
      }
    }
  }

  private async sendToSplunk(event: PipelineEvent, config: any): Promise<void> {
    // Send to Splunk HTTP Event Collector
    const response = await fetch(config.endpoint, {
      method: "POST",
      headers: {
        "Authorization": `Splunk ${config.token}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify(event),
    });
    if (!response.ok) throw new Error(`Splunk error: ${response.status}`);
  }

  private async sendToDatadog(event: PipelineEvent, config: any): Promise<void> {
    // Send to Datadog API
    const response = await fetch(config.endpoint, {
      method: "POST",
      headers: {
        "DD-API-KEY": config.apiKey,
        "Content-Type": "application/json",
      },
      body: JSON.stringify(event),
    });
    if (!response.ok) throw new Error(`Datadog error: ${response.status}`);
  }

  private async sendToKinesis(event: PipelineEvent, config: any): Promise<void> {
    // Send to AWS Kinesis (would use AWS SDK in production)
    // For demo, simulate
    throw new Error("Kinesis not implemented in demo");
  }

  private async sendToS3(event: PipelineEvent, config: any): Promise<void> {
    // Send to S3 (would use AWS SDK in production)
    // For demo, simulate
    throw new Error("S3 not implemented in demo");
  }

  private async sendToElasticsearch(event: PipelineEvent, config: any): Promise<void> {
    // Send to Elasticsearch
    const response = await fetch(config.endpoint, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify(event),
    });
    if (!response.ok) throw new Error(`Elasticsearch error: ${response.status}`);
  }

  private async storePendingEvents(events: PipelineEvent[]): Promise<void> {
    const pending = await this.state.storage.get<PipelineEvent[]>("pending") || [];
    pending.push(...events);
    await this.state.storage.put("pending", pending);
    await this.state.storage.put("pending_count", pending.length);
  }

  private async storeFailedEvent(event: PipelineEvent, dest: DestinationConfig): Promise<void> {
    const failed = await this.state.storage.get<any[]>("failed") || [];
    failed.push({ event, destination: dest, timestamp: new Date() });
    await this.state.storage.put("failed", failed);
  }
}

/**
 * Ingestion Worker (runs in V8 isolate)
 */
export class IngestionWorker {
  private pipelineProcessor: DurableObjectNamespace;

  constructor(pipelineProcessor: DurableObjectNamespace) {
    this.pipelineProcessor = pipelineProcessor;
  }

  async ingest(events: PipelineEvent[]): Promise<void> {
    const id = this.pipelineProcessor.idFromName("pipeline-processor");
    const stub = this.pipelineProcessor.get(id);

    const response = await stub.fetch("https://pipeline/process", {
      method: "POST",
      body: JSON.stringify(events),
    });

    if (!response.ok) {
      throw new Error(`Pipeline processing failed: ${response.status}`);
    }
  }
}

/**
 * Main worker entry point
 */
export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
    
    // Route to Pipeline Processor Durable Object
    if (url.pathname.startsWith("/pipeline")) {
      const id = env.PIPELINE_PROCESSOR.idFromName("pipeline-processor");
      const stub = env.PIPELINE_PROCESSOR.get(id);
      return stub.fetch(request);
    }

    return new Response("Not found", { status: 404 });
  },
};















