"""
Ray Parameter Server Actors - Distributed ML Training (Python WASM with SDK)

Demonstrates distributed ML training with parameter server pattern:
- Centralized model weights management
- Gradient aggregation from multiple workers
- Synchronous/asynchronous training patterns

Real-world use case: Distributed ML training (TensorFlow, PyTorch, Ray style).

## SDK Features Used

- @actor: Marks classes as PlexSpaces actors
- state(): Defines persistent state
- @handler(): Routes operations
"""

import json
from typing import List, Dict, Any
from plexspaces import actor, state, handler, init_handler, host


@actor
class ParameterServer:
    """Parameter server for distributed ML training (Ray-style)."""
    
    # Model weights (simplified 2-layer neural network)
    # w1: 200x784 (hidden layer: 784 inputs -> 200 hidden units)
    # w2: 200 (output layer: 200 hidden -> 1 output)
    w1: List[List[float]] = state(default_factory=lambda: [[0.1] * 784 for _ in range(200)])
    w2: List[float] = state(default_factory=lambda: [0.1] * 200)
    
    # Training state
    learning_rate: float = state(default=0.01)
    iteration: int = state(default=0)
    
    @init_handler
    def on_init(self, config: dict):
        """Initialize parameter server from config."""
        self.learning_rate = config.get("learning_rate", 0.01)
        self.iteration = 0
        # Initialize weights if not already set
        if not self.w1 or len(self.w1) == 0:
            self.w1 = [[0.1] * 784 for _ in range(200)]
        if not self.w2 or len(self.w2) == 0:
            self.w2 = [0.1] * 200
        host.info(f"Parameter server initialized: lr={self.learning_rate}")
    
    @handler("get_weights")
    def get_weights(self) -> dict:
        """Get current model weights."""
        return {
            "status": "ok",
            "weights": {
                "w1": self.w1,
                "w2": self.w2
            },
            "iteration": self.iteration
        }
    
    @handler("apply_gradients")
    def apply_gradients(self, gradients: List[Dict[str, Any]] = None) -> dict:
        """
        Apply gradients from multiple workers (aggregate and update weights).
        
        Args:
            gradients: List of gradient dictionaries, each with d_w1 and d_w2
        """
        if not gradients or len(gradients) == 0:
            return {"status": "error", "error": "No gradients provided"}
        
        num_workers = len(gradients)
        host.info(f"Applying gradients from {num_workers} workers (iteration {self.iteration})")
        
        # Aggregate gradients (sum across all workers)
        aggregated_d_w1 = [[0.0] * 784 for _ in range(200)]
        aggregated_d_w2 = [0.0] * 200
        
        for grad in gradients:
            d_w1 = grad.get("d_w1", [])
            d_w2 = grad.get("d_w2", [])
            
            # Sum gradients
            for i in range(200):
                for j in range(784):
                    if i < len(d_w1) and j < len(d_w1[i]):
                        aggregated_d_w1[i][j] += d_w1[i][j]
                if i < len(d_w2):
                    aggregated_d_w2[i] += d_w2[i]
        
        # Average gradients (divide by number of workers)
        for i in range(200):
            for j in range(784):
                aggregated_d_w1[i][j] /= num_workers
            aggregated_d_w2[i] /= num_workers
        
        # Update weights using SGD: w = w - lr * grad
        for i in range(200):
            for j in range(784):
                self.w1[i][j] -= self.learning_rate * aggregated_d_w1[i][j]
            self.w2[i] -= self.learning_rate * aggregated_d_w2[i]
        
        self.iteration += 1
        
        return {
            "status": "ok",
            "weights": {
                "w1": self.w1,
                "w2": self.w2
            },
            "iteration": self.iteration,
            "num_workers": num_workers
        }
    
    @handler("stats")
    def get_stats(self) -> dict:
        """Get training statistics."""
        # Calculate weight statistics
        w1_flat = [w for row in self.w1 for w in row]
        w1_mean = sum(w1_flat) / len(w1_flat) if w1_flat else 0.0
        w1_max = max(w1_flat) if w1_flat else 0.0
        w1_min = min(w1_flat) if w1_flat else 0.0
        
        w2_mean = sum(self.w2) / len(self.w2) if self.w2 else 0.0
        w2_max = max(self.w2) if self.w2 else 0.0
        w2_min = min(self.w2) if self.w2 else 0.0
        
        return {
            "status": "ok",
            "iteration": self.iteration,
            "learning_rate": self.learning_rate,
            "w1_stats": {
                "mean": w1_mean,
                "max": w1_max,
                "min": w1_min,
                "shape": [len(self.w1), len(self.w1[0]) if self.w1 else 0]
            },
            "w2_stats": {
                "mean": w2_mean,
                "max": w2_max,
                "min": w2_min,
                "shape": [len(self.w2)]
            }
        }


@actor
class DataWorker:
    """Data worker for distributed gradient computation (Ray-style)."""
    
    # Worker state
    worker_id: str = state(default="")
    data_shard: List[List[float]] = state(default_factory=list)  # List of [input_vector, target]
    batch_size: int = state(default=128)  # Increased from 32 to 128 for better benchmarks
    
    @init_handler
    def on_init(self, config: dict):
        """Initialize data worker from config."""
        self.worker_id = config.get("worker_id", "")
        self.batch_size = config.get("batch_size", 32)
        
        # Generate synthetic data shard if not provided
        # Use larger shard size for non-trivial benchmarks (runs for several seconds)
        if not self.data_shard:
            shard_size = config.get("shard_size", 5000)  # Increased from 1000 to 5000
            self.data_shard = []
            for i in range(shard_size):
                # Generate synthetic input (784 features) and target
                input_vec = [(i + j) * 0.001 for j in range(784)]
                target = float(i % 10)
                self.data_shard.append([input_vec, target])
        
        host.info(f"Data worker {self.worker_id} initialized: {len(self.data_shard)} samples")
    
    @handler("compute_gradients")
    def compute_gradients(self, weights: Dict[str, Any] = None) -> dict:
        """
        Compute gradients on this worker's data shard.
        
        Args:
            weights: Model weights dictionary with w1 (200x784) and w2 (200)
        
        Returns:
            Gradients dictionary with d_w1 and d_w2
        """
        if not weights:
            return {"status": "error", "error": "No weights provided"}
        
        w1 = weights.get("w1", [])
        w2 = weights.get("w2", [])
        
        if not w1 or not w2:
            return {"status": "error", "error": "Invalid weights structure"}
        
        host.info(f"[Worker {self.worker_id}] Computing gradients on {len(self.data_shard)} samples")
        
        # Initialize gradient accumulators
        d_w1 = [[0.0] * 784 for _ in range(200)]
        d_w2 = [0.0] * 200
        
        # Process batch (simulate gradient computation)
        # In real ML training, this would do forward/backward pass
        batch = self.data_shard[:self.batch_size]
        
        for sample in batch:
            input_vec = sample[0] if len(sample) > 0 else []
            target = sample[1] if len(sample) > 1 else 0.0
            
            # Simulate gradient computation (simplified)
            # Real implementation would compute actual gradients via backpropagation
            for i in range(200):
                for j in range(min(784, len(input_vec))):
                    # Simulated gradient: simplified computation
                    d_w1[i][j] += input_vec[j] * 0.001
                # Simulated gradient for output layer
                d_w2[i] += 0.001
        
        # Average gradients over batch
        if batch:
            for i in range(200):
                for j in range(784):
                    d_w1[i][j] /= len(batch)
                d_w2[i] /= len(batch)
        
        return {
            "status": "ok",
            "gradients": {
                "d_w1": d_w1,
                "d_w2": d_w2
            },
            "worker_id": self.worker_id,
            "samples_processed": len(batch)
        }
    
    @handler("stats")
    def get_stats(self) -> dict:
        """Get worker statistics."""
        return {
            "status": "ok",
            "worker_id": self.worker_id,
            "shard_size": len(self.data_shard),
            "batch_size": self.batch_size
        }
