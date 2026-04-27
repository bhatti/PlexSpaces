// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Native Orleans Implementation - Batch Predictor Grain
//
// This file demonstrates how batch prediction with model caching
// would be implemented in Microsoft Orleans (C#).
//
// Key Features:
// - Virtual Grains: Automatic activation/deactivation
// - Model Caching: Model loaded once per grain, cached in memory
// - Timers: Built-in RegisterTimer for periodic operations
// - Reminders: Built-in RegisterReminder for durable scheduled jobs

using Orleans;
using System;
using System.Collections.Generic;
using System.Threading.Tasks;

namespace Orleans.BatchPredictor
{
    /// <summary>
    /// Batch Predictor Grain Interface (Orleans)
    /// </summary>
    public interface IBatchPredictorGrain : IGrainWithStringKey
    {
        Task LoadModel(string modelId);
        Task<List<Prediction>> PredictBatch(string shardPath, List<DataPoint> data);
        Task StartPeriodicBatch(int intervalSecs);
        Task ScheduleBatchJob(string jobId, DateTime scheduledTime);
        Task<Stats> GetStats();
    }

    /// <summary>
    /// Data Point for ML inference
    /// </summary>
    public class DataPoint
    {
        public string Id { get; set; }
        public List<double> Features { get; set; }
    }

    /// <summary>
    /// Prediction Result
    /// </summary>
    public class Prediction
    {
        public string DataId { get; set; }
        public int Score { get; set; }
        public long Timestamp { get; set; }
    }

    /// <summary>
    /// Statistics
    /// </summary>
    public class Stats
    {
        public long ProcessedCount { get; set; }
        public bool ModelLoaded { get; set; }
        public string ModelId { get; set; }
        public int ModelSizeMb { get; set; }
    }

    /// <summary>
    /// ML Model (simulated)
    /// </summary>
    public class MLModel
    {
        public string ModelId { get; set; }
        public int SizeMb { get; set; }

        public static MLModel Load(string modelId)
        {
            // In production, load from storage (S3, HDFS, etc.)
            return new MLModel
            {
                ModelId = modelId,
                SizeMb = 10, // Simulated model size
            };
        }

        public List<Prediction> Predict(List<DataPoint> data)
        {
            var predictions = new List<Prediction>();
            foreach (var point in data)
            {
                // Simulated ML inference (sum features, compute score)
                var sum = 0.0;
                foreach (var feature in point.Features ?? new List<double>())
                {
                    sum += feature;
                }
                predictions.Add(new Prediction
                {
                    DataId = point.Id ?? $"data-{predictions.Count}",
                    Score = (int)(sum % 2), // Binary classification
                    Timestamp = 1771041911, // Fixed timestamp
                });
            }
            return predictions;
        }
    }

    /// <summary>
    /// Batch Predictor Grain Implementation (Orleans)
    /// </summary>
    public class BatchPredictorGrain : Grain, IBatchPredictorGrain
    {
        private MLModel _model;
        private long _processedCount = 0;
        private string _modelId = null;
        private bool _modelLoaded = false;

        /// <summary>
        /// Orleans automatically activates grain on first message
        /// </summary>
        public override Task OnActivateAsync()
        {
            // Grain activated - initialize state
            _processedCount = 0;
            _modelLoaded = false;
            _modelId = null;
            return base.OnActivateAsync();
        }

        /// <summary>
        /// Load model and cache in grain (Orleans: LoadModel)
        /// Model is loaded once and reused for all subsequent predictions
        /// </summary>
        public Task LoadModel(string modelId)
        {
            // Load model once, cache in grain (persists until grain deactivates)
            _model = MLModel.Load(modelId); // Load from storage (S3, HDFS, etc.)
            _modelId = modelId;
            _modelLoaded = true;
            return Task.CompletedTask;
        }

        /// <summary>
        /// Process batch prediction (Orleans: PredictBatch)
        /// Model is already cached, no reload needed
        /// </summary>
        public Task<List<Prediction>> PredictBatch(string shardPath, List<DataPoint> data)
        {
            // Model already cached - no reload needed
            if (!_modelLoaded)
            {
                // Auto-load default model if not loaded
                _model = MLModel.Load("default-model");
                _modelLoaded = true;
                _modelId = "default-model";
            }

            // Process predictions using cached model
            var predictions = _model.Predict(data);
            _processedCount += predictions.Count;
            return Task.FromResult(predictions);
        }

        /// <summary>
        /// Get statistics (Orleans: GetStats)
        /// </summary>
        public Task<Stats> GetStats()
        {
            return Task.FromResult(new Stats
            {
                ProcessedCount = _processedCount,
                ModelLoaded = _modelLoaded,
                ModelId = _modelId,
                ModelSizeMb = _model?.SizeMb ?? 0,
            });
        }

        /// <summary>
        /// Start periodic batch processing (Orleans: StartPeriodicBatch)
        /// Uses Orleans built-in timer registration
        /// </summary>
        public Task StartPeriodicBatch(int intervalSecs)
        {
            // Orleans built-in timer registration
            RegisterTimer(async _ =>
            {
                await ProcessPeriodicBatch();
            }, null,
            TimeSpan.FromSeconds(1), // Initial delay
            TimeSpan.FromSeconds(intervalSecs)); // Periodic interval
            return Task.CompletedTask;
        }

        /// <summary>
        /// Schedule batch job (Orleans: ScheduleBatchJob)
        /// Uses Orleans built-in reminder registration (durable)
        /// </summary>
        public async Task ScheduleBatchJob(string jobId, DateTime scheduledTime)
        {
            // Orleans built-in reminder registration (durable)
            await RegisterOrUpdateReminder(
                jobId,
                TimeSpan.FromSeconds(10), // Due time
                TimeSpan.FromSeconds(30)); // Period
        }

        /// <summary>
        /// Process periodic batch (called by timer)
        /// </summary>
        private async Task ProcessPeriodicBatch()
        {
            // Periodic batch processing logic
            var data = await FetchBatchData();
            await PredictBatch("periodic-batch", data);
        }

        /// <summary>
        /// Fetch batch data (simulated)
        /// </summary>
        private Task<List<DataPoint>> FetchBatchData()
        {
            // In production, fetch from storage (S3, HDFS, etc.)
            return Task.FromResult(new List<DataPoint>
            {
                new DataPoint { Id = "periodic-1", Features = new List<double> { 1.0, 2.0, 3.0 } },
                new DataPoint { Id = "periodic-2", Features = new List<double> { 4.0, 5.0, 6.0 } },
            });
        }
    }
}
