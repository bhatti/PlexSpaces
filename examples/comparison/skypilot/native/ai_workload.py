# SkyPilot AI Workload Orchestration (Python)
# Demonstrates multi-cloud AI workload scheduling

import sky

# Define AI workload
@sky.task
def train_model():
    import torch
    # Training code here
    pass

# Run on cheapest GPU across clouds
job = sky.launch(
    train_model,
    resources=sky.Resources(accelerators="V100:1"),
    # SkyPilot automatically finds cheapest GPU across AWS, GCP, Azure
)

# Or specify cloud preference
job = sky.launch(
    train_model,
    resources=sky.Resources(accelerators="V100:1", cloud="aws"),
)

# Multi-cloud workload
@sky.task
def distributed_training():
    # Distributed training across multiple clouds
    pass

job = sky.launch(
    distributed_training,
    resources=sky.Resources(accelerators="V100:4", use_spot=True),  # Spot instances for cost savings
)
