# Merlin Ensemble Workflow (Python)
# Demonstrates HPC workflow orchestration for scientific simulation ensembles

from merlin import Workflow, Task

# Define ensemble workflow
workflow = Workflow("ensemble_simulation")

# Add simulation tasks (Merlin: enqueues 40M simulations in 100 seconds)
for i in range(num_simulations):
    task = Task(
        name=f"simulation_{i}",
        command="python run_simulation.py",
        resources={"cpu": 4, "memory": "8GB"},
    )
    workflow.add_task(task)

# Execute ensemble
workflow.execute()

# Aggregate results
results = workflow.get_results()
aggregated = aggregate_simulation_results(results)
