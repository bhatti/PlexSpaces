// Orleans Batch Predictor Grain (C#)
// Demonstrates model caching in virtual actors for batch prediction
// Based on: https://docs.ray.io/en/latest/ray-core/examples/batch_prediction.html

using Orleans;
using System;
using System.Collections.Generic;
using System.Threading.Tasks;

public interface IBatchPredictorGrain : IGrainWithStringKey
{
    Task LoadModel(string modelId);
    Task<List<Prediction>> PredictBatch(string shardPath, List<DataPoint> data);
    Task StartPeriodicBatch(int intervalSecs);
    Task ScheduleBatchJob(string jobId, DateTime scheduledTime);
    Task<Stats> GetStats();
}

public class BatchPredictorGrain : Grain, IBatchPredictorGrain
{
    private MLModel _model;
    private long _processedCount = 0;

    public Task LoadModel(string modelId)
    {
        // Load model once and cache in grain
        // Model is reused for all subsequent predictions
        _model = MLModel.Load(modelId);
        return Task.CompletedTask;
    }

    public Task<List<Prediction>> PredictBatch(string shardPath, List<DataPoint> data)
    {
        // Model is already cached, no reload needed
        var predictions = _model.Predict(data);
        _processedCount += predictions.Count;
        return Task.FromResult(predictions);
    }

    public Task StartPeriodicBatch(int intervalSecs)
    {
        // Timer: In-memory, lost on deactivation
        RegisterTimer(
            asyncCallback: async _ =>
            {
                // Process periodic batch
                await ProcessPeriodicBatch();
            },
            state: null,
            dueTime: TimeSpan.FromSeconds(1),
            period: TimeSpan.FromSeconds(intervalSecs));
        
        return Task.CompletedTask;
    }

    public async Task ScheduleBatchJob(string jobId, DateTime scheduledTime)
    {
        // Reminder: Durable, survives deactivation
        await RegisterOrUpdateReminder(
            jobId,
            TimeSpan.FromSeconds(10),
            TimeSpan.FromSeconds(30));
    }

    public Task<Stats> GetStats()
    {
        return Task.FromResult(new Stats
        {
            ProcessedCount = _processedCount,
            ModelLoaded = _model != null
        });
    }

    private Task ProcessPeriodicBatch()
    {
        // Periodic batch processing logic
        return Task.CompletedTask;
    }
}

// Usage:
// var predictor = grainFactory.GetGrain<IBatchPredictorGrain>("model-1");
// await predictor.LoadModel("ml-model-v1");
// var predictions = await predictor.PredictBatch("s3://bucket/data/shard-1.parquet", data);
