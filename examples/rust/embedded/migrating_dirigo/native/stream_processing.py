# Dirigo Stream Processing (Python)
# Demonstrates virtual actors for stream processing operators
# Based on: https://arxiv.org/abs/2308.03615

from dirigo import VirtualActor, StreamOperator
import time

# Create stream operators (virtual actors)
map_operator = VirtualActor.create(
    operator_type="map",
    config={"function": "transform"}
)

filter_operator = VirtualActor.create(
    operator_type="filter",
    config={"threshold": 50.0}
)

reduce_operator = VirtualActor.create(
    operator_type="reduce",
    config={"window_size": 5}
)

# Process sensor events in real-time
def process_sensor_stream(sensor_events):
    for event in sensor_events:
        # Map: Transform event
        transformed = map_operator.process(event)
        
        # Filter: Filter by threshold
        if transformed.value > 50.0:
            filtered = filter_operator.process(transformed)
            
            # Reduce: Aggregate in window
            aggregated = reduce_operator.process(filtered)
            
            # Alert if aggregated value exceeds threshold
            if aggregated.value > 80.0:
                send_alert(aggregated)

# Example sensor events
sensor_events = [
    {"event_id": "event-1", "sensor_id": "sensor-001", "sensor_type": "temperature", "value": 75.5},
    {"event_id": "event-2", "sensor_id": "sensor-002", "sensor_type": "pressure", "value": 45.2},
    {"event_id": "event-3", "sensor_id": "sensor-003", "sensor_type": "temperature", "value": 85.0},
]

# Process stream
process_sensor_stream(sensor_events)

# Virtual actors are automatically activated/deactivated
# Time-sharing compute resources for efficient processing
