# FaaS Actor Example

Demonstrates FaaS-style actor invocation via HTTP GET/POST requests using the InvokeActor RPC.

## Overview

This example shows how to:
1. Create a counter actor that responds to increment (POST) and get (GET) operations
2. Spawn the actor on a node
3. Register the actor with type information for efficient lookup
4. Invoke the actor via HTTP using the InvokeActor RPC:
   - `GET /api/v1/actors/{tenant_id}/{namespace}/counter` - Get counter value (ask pattern)
   - `POST /api/v1/actors/{tenant_id}/{namespace}/counter` - Increment counter (tell pattern)
   - `GET /api/v1/actors/{namespace}/counter` - Get counter value without tenant_id (defaults to "default")
   - `POST /api/v1/actors/{namespace}/counter` - Increment counter without tenant_id (defaults to "default")

## Running

```bash
cd examples/simple/faas_actor
CARGO_TARGET_DIR=../../../target cargo run --release
```

Or use the test script:

```bash
./test.sh
```

## Testing

The example will:
1. Start a node on port 8000
2. Spawn a counter actor
3. Test GET requests to read the counter
4. Test POST requests to increment the counter
5. Verify the counter value after multiple increments

## HTTP API

### GET - Read Counter
```bash
# With tenant_id and namespace
curl "http://localhost:8080/api/v1/actors/default/default/counter?action=get"

# Without tenant_id (defaults to "default")
curl "http://localhost:8080/api/v1/actors/default/counter?action=get"
```

Response:
```json
{
  "success": true,
  "payload": "{\"count\":0,\"action\":\"get\"}",
  "actor_id": "counter-1@counter-node",
  "error_message": ""
}
```

### POST - Increment Counter
```bash
# With tenant_id and namespace
curl -X POST "http://localhost:8080/api/v1/actors/default/default/counter" \
  -H "Content-Type: application/json" \
  -d '{"action":"increment"}'

# Without tenant_id (defaults to "default")
curl -X POST "http://localhost:8080/api/v1/actors/default/counter" \
  -H "Content-Type: application/json" \
  -d '{"action":"increment"}'
```

Response:
```json
{
  "success": true,
  "payload": "",
  "actor_id": "counter-1@counter-node",
  "error_message": ""
}
```

## Architecture

- **GET/DELETE requests**: Use `ask()` pattern (request-reply) - query params converted to JSON payload
- **POST/PUT requests**: Use `tell()` pattern (fire-and-forget) - request body becomes payload
- **Actor lookup**: Uses efficient O(1) hashmap lookup by `(tenant_id, namespace, actor_type)`
- **Load balancing**: Random selection when multiple actors of same type found
- **Tenant isolation**: All actors have tenant_id (default: "default" if not provided)
- **Namespace support**: All actors organized by namespace (default: "default" if not provided)
- **URI information**: `message.uri_path` and `message.uri_method` populated for HTTP-based invocations
- **Path routing**: Full HTTP path and subpath available in message metadata for custom routing

## FaaS and Serverless Integration

This example demonstrates FaaS-style actor invocation, which enables:

- **AWS Lambda Function URLs**: Ready for integration with AWS Lambda
- **API Gateway**: Works with AWS API Gateway, Azure API Management, etc.
- **Serverless Patterns**: Automatic load balancing and scaling
- **RESTful API**: Standard HTTP methods for actor invocation

See the main documentation for detailed information on:
- [FaaS Invocation](../docs/concepts.md#faas-style-invocation) - Core concepts
- [Architecture](../docs/architecture.md#faas-invocation) - System design
- [Detailed Design](../docs/detailed-design.md#invokeactor-service) - Implementation details
- [Use Cases](../docs/use-cases.md#faas-platforms) - Real-world applications
