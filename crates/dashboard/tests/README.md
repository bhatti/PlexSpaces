# Dashboard Integration Tests

## Overview

Comprehensive integration tests for the Dashboard Service that test all dashboard methods with real node instances, applications, actors, and clusters.

## Test File

- `dashboard_integration_tests.rs` - Full integration tests for all dashboard methods
- `dashboard_service_tests.rs` - Unit tests for dashboard service

## Running Tests

```bash
# Run all integration tests
cargo test -p plexspaces-dashboard --test dashboard_integration_tests

# Run specific test
cargo test -p plexspaces-dashboard --test dashboard_integration_tests test_get_summary_with_single_node

# Run with output
cargo test -p plexspaces-dashboard --test dashboard_integration_tests -- --nocapture

# Run single-threaded (recommended to avoid port conflicts)
cargo test -p plexspaces-dashboard --test dashboard_integration_tests -- --test-threads=1
```

## Test Coverage

### Summary Tests
- ✅ `test_get_summary_with_single_node` - Basic summary with single node
- ✅ `test_get_summary_with_applications` - Summary with multiple applications
- ✅ `test_get_summary_comprehensive` - Comprehensive test with apps and actors

### Node Tests
- ✅ `test_get_nodes_single_node` - Get single node
- ✅ `test_get_nodes_with_pagination` - Pagination support

### Node Dashboard Tests
- ✅ `test_get_node_dashboard` - Full node dashboard with apps

### Application Tests
- ✅ `test_get_applications_empty` - Empty applications list
- ✅ `test_get_applications_with_multiple_apps` - Multiple applications
- ✅ `test_get_applications_with_name_pattern_filter` - Name pattern filtering
- ✅ `test_get_applications_pagination` - Pagination with many apps

### Actor Tests
- ✅ `test_get_actors_empty` - Empty actors list

### Workflow Tests
- ✅ `test_get_workflows` - Workflows list (may be empty if WorkflowService not registered)

### Dependency Health Tests
- ✅ `test_get_dependency_health` - Dependency health status

## Test Scenarios

### Simulated Scenarios

1. **Single Node**: Basic node with no applications or actors
2. **Multiple Applications**: Node with 3-15 registered applications
3. **Multiple Actors**: Node with actors of different types (calculator, processor, worker)
4. **Pagination**: Testing with page_size limits
5. **Filtering**: Name patterns, tenant IDs, cluster IDs
6. **Comprehensive**: Node with apps, actors, and all features enabled

### Test Helpers

- `create_test_node(node_id)` - Creates a test node with all services initialized
- `create_dashboard_service(node)` - Creates dashboard service from a node
- `register_application(node, name, version)` - Registers a mock application

## Validation

All tests validate:
- ✅ Correct response structure
- ✅ Expected counts (nodes, clusters, tenants, applications)
- ✅ Pagination metadata
- ✅ Filtering behavior
- ✅ Error handling

## Example Validation

The tests validate that:
- Empty nodes show at least 1 node, 1 cluster, 1 tenant
- Applications are correctly counted and filtered
- Pagination respects page_size
- Name pattern filtering works
- Node dashboard includes all expected sections

