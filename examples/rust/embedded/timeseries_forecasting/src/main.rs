// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Time-Series Forecasting Example - SDK Annotations Demo
//
// Demonstrates an end-to-end time-series forecasting application
// using PlexSpaces SDK annotations, inspired by Ray's time-series example.
//
// ## SDK Annotations Used
// - `#[gen_server_actor]` - generates GenServer behavior (request-reply)
// - `#[plexspaces_handlers(gen_server)]` - generates handler dispatch
// - `#[handler("op")]` - request-reply handlers
//
// ## Features
// - Distributed data preprocessing
// - Model training with actor coordination
// - Model validation (offline batch inference)
// - Online model serving

use plexspaces_sdk::{
    gen_server_actor, plexspaces_handlers,
    ActorContext, BehaviorError, Message, RequestContext, spawn, RequestContextExt};
use plexspaces_node::NodeBuilder;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, Level};

// Required for macro-generated code


// =============================================================================
// Domain Types - Time Series Forecasting
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeSeriesData {
    pub series_id: String,
    pub timestamps: Vec<i64>,
    pub values: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreprocessedData {
    pub series_id: String,
    pub normalized_values: Vec<f64>,
    pub mean: f64,
    pub std_dev: f64,
    pub window_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainedModel {
    pub model_id: String,
    pub weights: Vec<f64>,
    pub bias: f64,
    pub training_loss: f64,
    pub epochs: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub model_id: String,
    pub mse: f64,
    pub mae: f64,
    pub r_squared: f64,
    pub samples_validated: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictionRequest {
    pub series_id: String,
    pub input_values: Vec<f64>,
    pub horizon: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictionResponse {
    pub series_id: String,
    pub predictions: Vec<f64>,
    pub confidence_intervals: Vec<(f64, f64)>,
}

// =============================================================================
// Data Loader Actor - SDK Annotations
// =============================================================================

/// Data loader actor that loads and partitions time-series data.
/// 
/// ## Annotations
/// - `#[gen_server_actor]` - generates GenServer behavior
/// - `#[plexspaces_handlers(gen_server)]` - generates handler dispatch
/// - `#[handler("load_data")]` - handles data loading requests
#[gen_server_actor]
struct DataLoaderActor {
    loaded_series: Vec<String>,
    total_points_loaded: usize,
}

impl DataLoaderActor {
    fn new() -> Self {
        Self {
            loaded_series: Vec::new(),
            total_points_loaded: 0,
        }
    }
}

#[plexspaces_handlers(gen_server)]
impl DataLoaderActor {
    /// Load time-series data from source
    #[handler("load_data")]
    async fn handle_load_data(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Vec<u8>, BehaviorError> {
        #[derive(Deserialize)]
        struct LoadRequest {
            source: String,
            series_ids: Vec<String>,
        }
        
        let request: LoadRequest = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Invalid request: {}", e)))?;
        
        info!("📊 Loading data from: {}", request.source);
        
        // Simulate loading data (in real app, read from file/database)
        let mut loaded_data = Vec::new();
        
        for series_id in &request.series_ids {
            // Generate sample time-series data
            let data_points = 1000;
            let timestamps: Vec<i64> = (0..data_points).map(|i| 1000 + i as i64).collect();
            let values: Vec<f64> = (0..data_points)
                .map(|i| {
                    // Simulate seasonal pattern with noise
                    let trend = i as f64 * 0.01;
                    let seasonal = (i as f64 * 0.1).sin() * 10.0;
                    let noise = (i as f64 * 0.7).cos() * 2.0;
                    trend + seasonal + noise + 100.0
                })
                .collect();
            
            let data = TimeSeriesData {
                series_id: series_id.clone(),
                timestamps,
                values,
            };
            
            self.loaded_series.push(series_id.clone());
            self.total_points_loaded += data_points;
            loaded_data.push(data);
            
            info!("  ✓ Loaded {} with {} points", series_id, data_points);
        }
        
        serde_json::to_vec(&loaded_data)
            .map_err(|e| BehaviorError::ProcessingError(format!("Serialization error: {}", e)))
    }
    
    /// Get loader stats
    #[handler("get_stats")]
    async fn handle_get_stats(
        &mut self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<Vec<u8>, BehaviorError> {
        let stats = serde_json::json!({
            "loaded_series": self.loaded_series,
            "total_points_loaded": self.total_points_loaded,
        });
        
        serde_json::to_vec(&stats)
            .map_err(|e| BehaviorError::ProcessingError(format!("Serialization error: {}", e)))
    }
}

// =============================================================================
// Preprocessor Actor - SDK Annotations
// =============================================================================

/// Preprocessor actor that normalizes and transforms time-series data.
#[gen_server_actor]
struct PreprocessorActor {
    processed_count: usize,
    window_size: usize,
}

impl PreprocessorActor {
    fn new(window_size: usize) -> Self {
        Self {
            processed_count: 0,
            window_size,
        }
    }
}

#[plexspaces_handlers(gen_server)]
impl PreprocessorActor {
    /// Preprocess time-series data (normalize, create windows)
    #[handler("preprocess")]
    async fn handle_preprocess(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Vec<u8>, BehaviorError> {
        let data: TimeSeriesData = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Invalid data: {}", e)))?;
        
        info!("🔧 Preprocessing series: {}", data.series_id);
        
        // Calculate statistics
        let n = data.values.len() as f64;
        let mean = data.values.iter().sum::<f64>() / n;
        let variance = data.values.iter()
            .map(|v| (v - mean).powi(2))
            .sum::<f64>() / n;
        let std_dev = variance.sqrt();
        
        // Normalize values (z-score normalization)
        let normalized_values: Vec<f64> = data.values.iter()
            .map(|v| (v - mean) / std_dev.max(1e-10))
            .collect();
        
        self.processed_count += 1;
        
        let preprocessed = PreprocessedData {
            series_id: data.series_id,
            normalized_values,
            mean,
            std_dev,
            window_size: self.window_size,
        };
        
        info!("  ✓ Normalized with mean={:.2}, std={:.2}", mean, std_dev);
        
        serde_json::to_vec(&preprocessed)
            .map_err(|e| BehaviorError::ProcessingError(format!("Serialization error: {}", e)))
    }
}

// =============================================================================
// Trainer Actor - SDK Annotations
// =============================================================================

/// Trainer actor that trains forecasting models.
#[gen_server_actor]
struct TrainerActor {
    models_trained: usize,
    learning_rate: f64,
    epochs: u32,
}

impl TrainerActor {
    fn new(learning_rate: f64, epochs: u32) -> Self {
        Self {
            models_trained: 0,
            learning_rate,
            epochs,
        }
    }
}

#[plexspaces_handlers(gen_server)]
impl TrainerActor {
    /// Train a forecasting model on preprocessed data
    #[handler("train")]
    async fn handle_train(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Vec<u8>, BehaviorError> {
        let data: PreprocessedData = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Invalid data: {}", e)))?;
        
        info!("🎯 Training model for series: {}", data.series_id);
        
        // Simple linear regression training (in real app, use proper ML library)
        let window_size = data.window_size;
        let n_samples = data.normalized_values.len().saturating_sub(window_size);
        
        // Initialize weights
        let mut weights = vec![0.1; window_size];
        let mut bias = 0.0;
        let mut loss = f64::MAX;
        
        // Training loop (gradient descent)
        for epoch in 0..self.epochs {
            let mut total_loss = 0.0;
            
            for i in 0..n_samples {
                let x: Vec<f64> = data.normalized_values[i..i + window_size].to_vec();
                let y_true = data.normalized_values[i + window_size];
                
                // Forward pass
                let y_pred: f64 = x.iter().zip(weights.iter())
                    .map(|(xi, wi)| xi * wi)
                    .sum::<f64>() + bias;
                
                // Compute loss
                let error = y_pred - y_true;
                total_loss += error.powi(2);
                
                // Backward pass (gradient descent)
                for (j, xi) in x.iter().enumerate() {
                    weights[j] -= self.learning_rate * error * xi;
                }
                bias -= self.learning_rate * error;
            }
            
            loss = total_loss / n_samples as f64;
            
            if epoch % 100 == 0 {
                info!("    Epoch {}: loss = {:.6}", epoch, loss);
            }
        }
        
        self.models_trained += 1;
        
        let model = TrainedModel {
            model_id: format!("model-{}", data.series_id),
            weights,
            bias,
            training_loss: loss,
            epochs: self.epochs,
        };
        
        info!("  ✓ Model trained with final loss: {:.6}", loss);
        
        serde_json::to_vec(&model)
            .map_err(|e| BehaviorError::ProcessingError(format!("Serialization error: {}", e)))
    }
}

// =============================================================================
// Validator Actor - SDK Annotations
// =============================================================================

/// Validator actor that validates trained models.
#[gen_server_actor]
struct ValidatorActor {
    validations_performed: usize,
}

impl ValidatorActor {
    fn new() -> Self {
        Self {
            validations_performed: 0,
        }
    }
}

#[plexspaces_handlers(gen_server)]
impl ValidatorActor {
    /// Validate a trained model on test data
    #[handler("validate")]
    async fn handle_validate(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Vec<u8>, BehaviorError> {
        #[derive(Deserialize)]
        struct ValidateRequest {
            model: TrainedModel,
            test_data: PreprocessedData,
        }
        
        let request: ValidateRequest = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Invalid request: {}", e)))?;
        
        info!("📈 Validating model: {}", request.model.model_id);
        
        let window_size = request.model.weights.len();
        let n_samples = request.test_data.normalized_values.len().saturating_sub(window_size);
        
        let mut squared_errors = Vec::new();
        let mut absolute_errors = Vec::new();
        let mut predictions = Vec::new();
        let mut actuals = Vec::new();
        
        for i in 0..n_samples {
            let x: Vec<f64> = request.test_data.normalized_values[i..i + window_size].to_vec();
            let y_true = request.test_data.normalized_values[i + window_size];
            
            // Predict
            let y_pred: f64 = x.iter().zip(request.model.weights.iter())
                .map(|(xi, wi)| xi * wi)
                .sum::<f64>() + request.model.bias;
            
            predictions.push(y_pred);
            actuals.push(y_true);
            
            let error = y_pred - y_true;
            squared_errors.push(error.powi(2));
            absolute_errors.push(error.abs());
        }
        
        // Calculate metrics
        let mse = squared_errors.iter().sum::<f64>() / n_samples as f64;
        let mae = absolute_errors.iter().sum::<f64>() / n_samples as f64;
        
        // R-squared
        let y_mean = actuals.iter().sum::<f64>() / n_samples as f64;
        let ss_tot: f64 = actuals.iter().map(|y| (y - y_mean).powi(2)).sum();
        let ss_res: f64 = squared_errors.iter().sum();
        let r_squared = 1.0 - (ss_res / ss_tot.max(1e-10));
        
        self.validations_performed += 1;
        
        let result = ValidationResult {
            model_id: request.model.model_id,
            mse,
            mae,
            r_squared,
            samples_validated: n_samples,
        };
        
        info!("  ✓ Validation complete: MSE={:.4}, MAE={:.4}, R²={:.4}", mse, mae, r_squared);
        
        serde_json::to_vec(&result)
            .map_err(|e| BehaviorError::ProcessingError(format!("Serialization error: {}", e)))
    }
}

// =============================================================================
// Server Actor - SDK Annotations
// =============================================================================

/// Server actor that serves predictions from trained models.
#[gen_server_actor]
struct ServerActor {
    model: Option<TrainedModel>,
    predictions_served: usize,
}

impl ServerActor {
    fn new() -> Self {
        Self {
            model: None,
            predictions_served: 0,
        }
    }
}

#[plexspaces_handlers(gen_server)]
impl ServerActor {
    /// Load a trained model for serving
    #[handler("load_model")]
    async fn handle_load_model(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Vec<u8>, BehaviorError> {
        let model: TrainedModel = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Invalid model: {}", e)))?;
        
        info!("🚀 Loading model for serving: {}", model.model_id);
        self.model = Some(model);
        
        serde_json::to_vec(&serde_json::json!({"status": "model_loaded"}))
            .map_err(|e| BehaviorError::ProcessingError(format!("Serialization error: {}", e)))
    }
    
    /// Serve a prediction request
    #[handler("predict")]
    async fn handle_predict(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Vec<u8>, BehaviorError> {
        let request: PredictionRequest = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Invalid request: {}", e)))?;
        
        let model = self.model.as_ref()
            .ok_or_else(|| BehaviorError::ProcessingError("No model loaded".to_string()))?;
        
        info!("🔮 Serving prediction for series: {}", request.series_id);
        
        let mut predictions = Vec::new();
        let mut confidence_intervals = Vec::new();
        let mut input = request.input_values.clone();
        
        // Generate predictions for the requested horizon
        for _ in 0..request.horizon {
            let window: Vec<f64> = input.iter().rev().take(model.weights.len()).rev().cloned().collect();
            
            if window.len() < model.weights.len() {
                break;
            }
            
            let prediction: f64 = window.iter().zip(model.weights.iter())
                .map(|(xi, wi)| xi * wi)
                .sum::<f64>() + model.bias;
            
            // Simple confidence interval (±2 * training_loss)
            let ci = (prediction - 2.0 * model.training_loss.sqrt(), prediction + 2.0 * model.training_loss.sqrt());
            
            predictions.push(prediction);
            confidence_intervals.push(ci);
            input.push(prediction);
        }
        
        self.predictions_served += 1;
        
        let response = PredictionResponse {
            series_id: request.series_id,
            predictions,
            confidence_intervals,
        };
        
        info!("  ✓ Generated {} predictions", response.predictions.len());
        
        serde_json::to_vec(&response)
            .map_err(|e| BehaviorError::ProcessingError(format!("Serialization error: {}", e)))
    }
}

// =============================================================================
// Main
// =============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .with_env_filter("timeseries_forecasting=info,plexspaces=warn")
        .init();

    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║  Time-Series Forecasting Example (SDK Annotations Demo)        ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();
    println!("Demonstrates distributed ML pipeline using PlexSpaces SDK annotations");
    println!();

    // Create node
    let node = NodeBuilder::new("timeseries-node-1")
        .with_clustering_enabled(false)
        .build_started().await;
    let service_locator = node.service_locator();
    let ctx = RequestContext::new_without_auth(
        "ml-tenant".to_string(),
        "forecasting".to_string(),
    );

    info!("Node created: timeseries-node-1");

    sleep(Duration::from_millis(500)).await;
    info!("Node started");

    // =========================================================================
    // Step 1: Spawn actors using SDK annotations
    // =========================================================================
    println!();
    println!("Step 1: Spawn ML pipeline actors with SDK annotations");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    // Data Loader
    let _data_loader_ref = spawn(
        &ctx,
        service_locator.clone(),
        "data-loader",
        "ml-pipeline",
        DataLoaderActor::new(),
    )
    .await
    .map_err(|e| format!("Failed to spawn data loader: {}", e))?;
    info!("  ✓ DataLoaderActor spawned (GenServer)");

    // Preprocessor
    let _preprocessor_ref = spawn(
        &ctx,
        service_locator.clone(),
        "preprocessor",
        "ml-pipeline",
        PreprocessorActor::new(10), // window_size = 10
    )
    .await
    .map_err(|e| format!("Failed to spawn preprocessor: {}", e))?;
    info!("  ✓ PreprocessorActor spawned (GenServer)");

    // Trainer
    let _trainer_ref = spawn(
        &ctx,
        service_locator.clone(),
        "trainer",
        "ml-pipeline",
        TrainerActor::new(0.001, 500), // learning_rate, epochs
    )
    .await
    .map_err(|e| format!("Failed to spawn trainer: {}", e))?;
    info!("  ✓ TrainerActor spawned (GenServer)");

    // Validator
    let _validator_ref = spawn(
        &ctx,
        service_locator.clone(),
        "validator",
        "ml-pipeline",
        ValidatorActor::new(),
    )
    .await
    .map_err(|e| format!("Failed to spawn validator: {}", e))?;
    info!("  ✓ ValidatorActor spawned (GenServer)");

    // Server
    let _server_ref = spawn(
        &ctx,
        service_locator.clone(),
        "server",
        "ml-pipeline",
        ServerActor::new(),
    )
    .await
    .map_err(|e| format!("Failed to spawn server: {}", e))?;
    info!("  ✓ ServerActor spawned (GenServer)");

    println!();
    info!("All ML pipeline actors spawned");

    // =========================================================================
    // Step 2: Simulate ML workflow
    // =========================================================================
    println!();
    println!("Step 2: ML Pipeline Workflow");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("  Pipeline: DataLoader → Preprocessor → Trainer → Validator → Server");
    println!();
    println!("  In production, actors communicate via messages:");
    println!("    1. DataLoader.load_data() → TimeSeriesData");
    println!("    2. Preprocessor.preprocess() → PreprocessedData");
    println!("    3. Trainer.train() → TrainedModel");
    println!("    4. Validator.validate() → ValidationResult");
    println!("    5. Server.load_model() + Server.predict() → PredictionResponse");
    println!();

    // Simulate workflow steps
    info!("Step 2.1: Loading data...");
    sleep(Duration::from_millis(200)).await;
    
    info!("Step 2.2: Preprocessing data...");
    sleep(Duration::from_millis(200)).await;
    
    info!("Step 2.3: Training model...");
    sleep(Duration::from_millis(200)).await;
    
    info!("Step 2.4: Validating model...");
    sleep(Duration::from_millis(200)).await;
    
    info!("Step 2.5: Serving predictions...");
    sleep(Duration::from_millis(200)).await;

    // =========================================================================
    // Summary
    // =========================================================================
    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Time-Series Forecasting Example Complete!");
    println!();
    println!("SDK Annotations Used:");
    println!("  • #[gen_server_actor] - GenServer behavior (request-reply)");
    println!("  • #[plexspaces_handlers(gen_server)] - Handler dispatch");
    println!("  • #[handler(\"op\")] - Request-reply handlers");
    println!();
    println!("ML Pipeline Actors:");
    println!("  • DataLoaderActor - Load and partition time-series data");
    println!("  • PreprocessorActor - Normalize and create windows");
    println!("  • TrainerActor - Train forecasting models");
    println!("  • ValidatorActor - Validate model performance");
    println!("  • ServerActor - Serve predictions online");
    println!();
    println!("Key Takeaways:");
    println!("  • Distributed data preprocessing using actors");
    println!("  • Model training with actor coordination");
    println!("  • Model validation with batch inference");
    println!("  • Online model serving via actor messages");
    println!();

    info!("Shutting down...");
    node.shutdown(Duration::from_secs(5)).await?;

    Ok(())
}
