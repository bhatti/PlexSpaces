# eFlows4HPC Unified Workflow (Python)
# Demonstrates unified workflow platform (HPC + Big Data + ML)

from eflows4hpc import Workflow, HCPStep, AnalyticsStep, MLStep

# Define unified workflow (HPC → Analytics → ML)
workflow = Workflow("unified_pipeline")

# HPC simulation step
hpc_step = HCPStep(
    step_id="simulation",
    step_type="climate",
    input_data=load_data(),
)

# Big data analytics step
analytics_step = AnalyticsStep(
    step_id="analysis",
    analytics_type="aggregation",
    query="SELECT * FROM simulation_results",
)

# ML training step
ml_step = MLStep(
    step_id="training",
    model_type="neural_network",
    training_data=analytics_output,
)

workflow.add_step(hpc_step)
workflow.add_step(analytics_step)
workflow.add_step(ml_step)
workflow.execute()
