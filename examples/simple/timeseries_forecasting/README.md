# Time-Series Forecasting Example

## Overview

This example demonstrates an end-to-end time-series forecasting application using PlexSpaces actors, inspired by [Ray's time-series forecasting example](https://docs.ray.io/en/latest/ray-overview/examples/e2e-timeseries/README.html).

## Features

This example showcases:

1. **Distributed Data Preprocessing**: Ingest and preprocess time-series data at scale using actor-based parallelism
2. **Model Training**: Train a distributed forecasting model using actor coordination
3. **Model Validation**: Evaluate the model using offline batch inference
4. **Online Model Serving**: Deploy the model as a scalable online service using actors

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Time-Series Forecasting                   │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────┐ │
│  │ Data Loader  │───▶│ Preprocessor │───▶│   Trainer    │ │
│  │   Actor      │    │   Actor      │    │   Actor      │ │
│  └──────────────┘    └──────────────┘    └──────────────┘ │
│         │                    │                    │         │
│         └────────────────────┴────────────────────┘         │
│                            │                                │
│                    ┌───────▼────────┐                       │
│                    │ Model Validator│                       │
│                    │     Actor      │                       │
│                    └───────┬────────┘                       │
│                            │                                │
│                    ┌───────▼────────┐                       │
│                    │ Model Server   │                       │
│                    │     Actor      │                       │
│                    └────────────────┘                       │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

## Components

### 1. Data Loader Actor
- Loads time-series data from files or streams
- Partitions data for parallel processing
- Sends data chunks to preprocessor actors

### 2. Preprocessor Actor
- Normalizes time-series data
- Handles missing values
- Creates sliding windows for training
- Sends preprocessed data to trainer

### 3. Trainer Actor
- Trains a simple linear forecasting model (DLinear-inspired)
- Coordinates distributed training across multiple actors
- Saves trained model state

### 4. Model Validator Actor
- Performs offline batch inference on test data
- Calculates metrics (MAE, RMSE, MAPE)
- Validates model performance

### 5. Model Server Actor
- Serves online predictions via actor messages
- Handles concurrent prediction requests
- Maintains model state for fast inference

## Usage

### Prerequisites

```bash
# Build the example
cd examples/simple/timeseries_forecasting
cargo build --release
```

### Run the Example

```bash
# Run single-node example
./scripts/run_single_node.sh

# Run multi-node example (requires Redis)
./scripts/run_multi_node.sh
```

### Test the Example

```bash
# Run unit tests
cargo test

# Run integration tests
cargo test --test integration
```

## Example Workflow

1. **Data Loading**: Load time-series data (e.g., sales, temperature, stock prices)
2. **Preprocessing**: Normalize and create training windows
3. **Training**: Train forecasting model on historical data
4. **Validation**: Evaluate model on test set
5. **Serving**: Serve predictions for new data points

## Key PlexSpaces Features Demonstrated

- **Actor-Based Parallelism**: Multiple actors process data in parallel
- **Message Passing**: Actors communicate via typed messages
- **State Management**: Actors maintain model state and data
- **Scalability**: Easy to scale by adding more actor instances
- **Fault Tolerance**: Actor supervision handles failures gracefully

## Comparison to Ray

This example demonstrates similar capabilities to Ray's time-series forecasting:

| Feature | Ray | PlexSpaces |
|---------|-----|------------|
| Data Preprocessing | Ray Data | Actor-based parallelism |
| Model Training | Ray Train | Actor coordination |
| Batch Inference | Ray Data | Actor-based batch processing |
| Online Serving | Ray Serve | Actor message handling |

## Next Steps

- Add more sophisticated models (LSTM, Transformer)
- Add distributed training with gradient aggregation
- Add model versioning and A/B testing
- Add monitoring and metrics collection

## References

- [Ray Time-Series Forecasting Example](https://docs.ray.io/en/latest/ray-overview/examples/e2e-timeseries/README.html)
- [DLinear Paper](https://arxiv.org/abs/2205.13504)
- [PlexSpaces Actor Model](../README.md)

