// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Orleans vs PlexSpaces Comparison - Batch Prediction with Model Caching
//
// Real-world use case: ML inference pipeline with virtual actors
// - Loads and caches ML models in actors (efficient reuse)
// - Processes large batches of data points (10K+)
// - Demonstrates virtual actor lifecycle with model caching
//
// Native Orleans: C# (Microsoft Orleans framework)
// PlexSpaces: TypeScript WASM actor using @plexspaces/sdk
//
// Architecture:
// - Facets (VirtualActorFacet, TimerFacet, ReminderFacet) configured via app-config.toml
// - Actor spawning handled by framework deployment (HTTP API)
// - Metrics provided by PlexSpaces runtime (coordination vs computation tracking)

import { PlexSpacesActor } from "@plexspaces/sdk";

interface DataPoint {
  id: string;
  features: number[];
}

interface Prediction {
  data_id: string;
  score: number;
  timestamp: number;
}

interface BatchPredictorState {
  model_id: string | null;
  model_loaded: boolean;
  processed_count: number;
  // Model payload (simulated - in production would be loaded from storage)
  model_payload_size_mb: number;
}

/**
 * Batch Predictor Actor - Orleans-style virtual actor with model caching
 *
 * Demonstrates:
 * - Model caching: Load once, reuse for all predictions
 * - Batch processing: Process large batches efficiently
 * - Virtual actor lifecycle: Automatic activation/deactivation
 *
 * Note: VirtualActorFacet, TimerFacet, ReminderFacet are configured
 * via app-config.toml and framework deployment, not SDK.
 */
export class BatchPredictorActor extends PlexSpacesActor<BatchPredictorState> {
  // Model payload is simulated (not actually allocated to avoid WASM memory limits)
  // In production, models would be loaded from external storage (S3, HDFS, etc.)

  getDefaultState(): BatchPredictorState {
    return {
      model_id: null,
      model_loaded: false,
      processed_count: 0,
      model_payload_size_mb: 0,
    };
  }

  protected override onInit(config: Record<string, unknown>): void {
    this.state.model_id = String(config.model_id ?? null);
    this.state.model_loaded = false;
    this.state.processed_count = 0;
    this.state.model_payload_size_mb = 0;
  }

  /**
   * Load model and cache in actor (Orleans: LoadModel)
   * Model is loaded once and reused for all subsequent predictions
   */
  onLoad_model(payload: Record<string, unknown>): Record<string, unknown> {
    try {
      // Safely extract model_id with minimal operations
      const modelId = payload && typeof payload === 'object' && 'model_id' in payload
        ? String(payload.model_id ?? "default-model")
        : "default-model";
      
      // Simulate loading a large model (10MB payload - reduced from 100MB to avoid WASM memory limits)
      // In production, this would load from storage (S3, HDFS, etc.)
      // Note: WASM components have limited memory, so we simulate without allocating
      // The actual model size is tracked in state for metrics, but we don't allocate any payload
      const modelSizeBytes = 10 * 1024 * 1024; // 10MB (reduced from 100MB)
      
      // No actual allocation - just track size for metrics
      // In production, models would be loaded from external storage on-demand
      
      // Update state with minimal operations
      this.state.model_id = modelId;
      this.state.model_loaded = true;
      this.state.model_payload_size_mb = modelSizeBytes / (1024 * 1024);
      
      // Return a version marker to verify new WASM is deployed
      // Use simple object literal to minimize memory allocation
      return {
        status: "ok",
        model_id: modelId,
        model_loaded: true,
        model_size_mb: this.state.model_payload_size_mb,
        wasm_version: "v2-no-memory-allocation", // Marker to verify new WASM is deployed
      };
    } catch (e) {
      // Catch any unexpected errors to prevent WASM trap
      // Return minimal error object to avoid memory issues
      const errorMsg = e instanceof Error ? e.message : String(e);
      return {
        status: "error",
        error: "load_model_failed",
        message: errorMsg.length > 100 ? errorMsg.substring(0, 100) : errorMsg, // Limit message length
      };
    }
  }

  /**
   * Process batch prediction (Orleans: PredictBatch)
   * Model is already cached, no reload needed
   */
  onPredict_batch(payload: Record<string, unknown>): Record<string, unknown> {
    if (!this.state.model_loaded) {
      return {
        status: "error",
        error: "model_not_loaded",
        message: "Model must be loaded before prediction",
      };
    }

    const startTime = Date.now();
    const dataPoints = Array.isArray(payload.data_points) ? payload.data_points as DataPoint[] : [];
    const predictions = this.processPredictions(dataPoints);
    const computeTimeMs = Date.now() - startTime;

    this.state.processed_count += predictions.length;

    return {
      status: "ok",
      shard_path: payload.shard_path ? String(payload.shard_path) : undefined,
      predictions: predictions,
      count: predictions.length,
      total_processed: this.state.processed_count,
      compute_time_ms: computeTimeMs,
    };
  }

  /**
   * Process predictions (simulated ML inference)
   * Matches Orleans pattern: model.Predict(data) → predictions
   * 
   * Note: No longer limited - iterative JSON serializer handles arrays safely
   */
  private processPredictions(dataPoints: DataPoint[]): Array<{data_id: string; score: number; timestamp: number}> {
    const predictions: Array<{data_id: string; score: number; timestamp: number}> = [];
    const dataPointsCount = dataPoints.length;
    
    for (let i = 0; i < dataPointsCount; i++) {
      const point = dataPoints[i];
      if (!point || typeof point !== 'object') {
        continue;
      }
      
      const features = Array.isArray(point.features) ? point.features : [];
      let sum = 0;
      const featuresCount = features.length;
      
      for (let j = 0; j < featuresCount; j++) {
        const val = features[j];
        if (typeof val === 'number') {
          sum += val;
        }
      }
      
      predictions.push({
        data_id: String(point.id ?? `data-${i}`),
        score: sum % 2, // Binary classification (0 or 1)
        timestamp: Date.now(),
      });
    }
    return predictions;
  }

  /**
   * Get statistics (Orleans: GetStats)
   */
  onGet_stats(): Record<string, unknown> {
    return {
      processed_count: this.state.processed_count,
      model_loaded: this.state.model_loaded,
      model_id: this.state.model_id,
      model_size_mb: this.state.model_payload_size_mb,
    };
  }

  /**
   * Start periodic batch processing (Orleans: StartPeriodicBatch)
   * Note: Timer registration handled by framework (TimerFacet via app-config.toml)
   */
  onStart_periodic_batch(payload: Record<string, unknown>): Record<string, unknown> {
    const intervalSecs = Number(payload.interval_secs ?? 5);
    
    // Timer registration is handled by framework (TimerFacet)
    // This handler just acknowledges the request
    return {
      status: "ok",
      interval_secs: intervalSecs,
      message: "Timer registration handled by framework (TimerFacet)",
    };
  }

  /**
   * Schedule batch job (Orleans: ScheduleBatchJob)
   * Note: Reminder registration handled by framework (ReminderFacet via app-config.toml)
   */
  onSchedule_batch_job(payload: Record<string, unknown>): Record<string, unknown> {
    const jobId = String(payload.job_id ?? "");
    // Use Math.floor to ensure integer timestamp
    const scheduledTime = Number(payload.scheduled_time ?? Math.floor(Date.now() / 1000));
    
    // Reminder registration is handled by framework (ReminderFacet)
    // This handler just acknowledges the request
    return {
      status: "ok",
      job_id: jobId,
      scheduled_time: scheduledTime,
      message: "Reminder registration handled by framework (ReminderFacet)",
    };
  }
}

// WIT actor export (used by component entry and verify)
const instance = new BatchPredictorActor();
export const actor = {
  init: (configJson: string) => {
    return instance.init(configJson);
  },
  handle: (from: string, msgType: string, payloadJson: string) => {
    return instance.handle(from, msgType, payloadJson);
  },
  getState: () => instance.getState(),
  setState: (stateJson: string) => instance.setState(stateJson),
};
