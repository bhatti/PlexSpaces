"""
Ray Parameter Server Example (Native Python)

This is the native Ray implementation for comparison.
Based on: https://docs.ray.io/en/latest/ray-core/examples/plot_parameter_server.html
"""

import ray
import numpy as np
from typing import List, Tuple


@ray.remote
class ParameterServer:
    """Ray parameter server for distributed ML training."""
    
    def __init__(self, learning_rate: float = 0.01):
        # Initialize model weights (simplified 2-layer network)
        # w1: 200x784 (hidden layer: 784 inputs -> 200 hidden units)
        # w2: 200 (output layer: 200 hidden -> 1 output)
        self.w1 = np.random.randn(200, 784) * 0.1
        self.w2 = np.random.randn(200) * 0.1
        self.learning_rate = learning_rate
        self.iteration = 0
    
    def apply_gradients(self, *gradients: List[np.ndarray]) -> Tuple[np.ndarray, np.ndarray]:
        """
        Apply gradients from multiple workers (aggregate and update weights).
        
        Args:
            *gradients: Variable number of gradient tuples (d_w1, d_w2) from workers
        
        Returns:
            Updated weights (w1, w2)
        """
        # Aggregate gradients (sum across all workers)
        aggregated_d_w1 = np.zeros_like(self.w1)
        aggregated_d_w2 = np.zeros_like(self.w2)
        
        num_workers = len(gradients)
        for grad_tuple in gradients:
            d_w1, d_w2 = grad_tuple
            aggregated_d_w1 += d_w1
            aggregated_d_w2 += d_w2
        
        # Average gradients
        aggregated_d_w1 /= num_workers
        aggregated_d_w2 /= num_workers
        
        # Update weights using SGD: w = w - lr * grad
        self.w1 -= self.learning_rate * aggregated_d_w1
        self.w2 -= self.learning_rate * aggregated_d_w2
        
        self.iteration += 1
        return self.w1, self.w2
    
    def get_weights(self) -> Tuple[np.ndarray, np.ndarray]:
        """Get current model weights."""
        return self.w1, self.w2


@ray.remote
class DataWorker:
    """Ray data worker for distributed gradient computation."""
    
    def __init__(self, worker_id: str, data_shard: List[Tuple[np.ndarray, float]], batch_size: int = 32):
        self.worker_id = worker_id
        self.data_shard = data_shard
        self.batch_size = batch_size
    
    def compute_gradients(self, weights: Tuple[np.ndarray, np.ndarray]) -> Tuple[np.ndarray, np.ndarray]:
        """
        Compute gradients on this worker's data shard.
        
        Args:
            weights: Model weights (w1, w2)
        
        Returns:
            Gradients (d_w1, d_w2)
        """
        w1, w2 = weights
        
        # Initialize gradient accumulators
        d_w1 = np.zeros_like(w1)
        d_w2 = np.zeros_like(w2)
        
        # Process batch (simulate gradient computation)
        batch = self.data_shard[:self.batch_size]
        
        for input_vec, target in batch:
            # Simulate gradient computation (simplified)
            # Real implementation would compute actual gradients via backpropagation
            d_w1 += input_vec.reshape(1, -1) * 0.001
            d_w2 += np.ones_like(w2) * 0.001
        
        # Average gradients over batch
        if batch:
            d_w1 /= len(batch)
            d_w2 /= len(batch)
        
        return d_w1, d_w2


def main():
    """Synchronous parameter server training (Ray pattern)."""
    ray.init()
    
    # Create parameter server
    ps = ParameterServer.remote(learning_rate=0.01)
    
    # Create elastic pool of data workers
    worker_count = 4
    workers = []
    
    for i in range(worker_count):
        # Generate synthetic data shard for each worker
        data_shard = [
            (np.random.randn(784), float(j % 10))
            for j in range(1000)
        ]
        worker = DataWorker.remote(
            worker_id=f"worker-{i}",
            data_shard=data_shard,
            batch_size=32
        )
        workers.append(worker)
    
    # Training loop
    iterations = 10
    
    # Get initial weights
    current_weights = ps.get_weights.remote()
    
    for iteration in range(iterations):
        print(f"[ITERATION {iteration + 1}] Starting training step")
        
        # Step 1: All workers compute gradients in parallel
        gradient_futures = [
            worker.compute_gradients.remote(current_weights)
            for worker in workers
        ]
        
        # Wait for all gradients
        gradients = ray.get(gradient_futures)
        
        # Step 2: Parameter server aggregates and applies gradients
        current_weights = ps.apply_gradients.remote(*gradients)
        
        print(f"[ITERATION {iteration + 1}] Updated model weights")
    
    # Get final weights
    final_weights = ray.get(current_weights)
    print(f"Training complete! Final weights shape: {final_weights[0].shape}, {final_weights[1].shape}")
    
    ray.shutdown()


if __name__ == "__main__":
    main()
