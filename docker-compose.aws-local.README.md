# AWS Local Testing Setup

This docker-compose file provides local instances of AWS services for development and testing.

## Services

### DynamoDB Local
- **Port**: 8000
- **Purpose**: Local DynamoDB emulator for testing
- **Used by**: locks, scheduler, keyvalue, workflow, journaling, tuplespace, blob metadata
- **Endpoint**: `http://localhost:8000`

### LocalStack
- **Port**: 4566 (edge port)
- **Purpose**: AWS service emulator (SQS, S3, etc.)
- **Used by**: channel (SQS), blob storage (S3)
- **Endpoint**: `http://localhost:4566`

## Usage

### Start Services
```bash
docker-compose -f docker-compose.aws-local.yml up -d
```

### Stop Services
```bash
docker-compose -f docker-compose.aws-local.yml down
```

### View Logs
```bash
docker-compose -f docker-compose.aws-local.yml logs -f
```

## Environment Variables

Set these environment variables to use local AWS services:

```bash
# AWS Region (required)
export AWS_REGION=us-east-1

# DynamoDB Local endpoint
export DYNAMODB_ENDPOINT_URL=http://localhost:8000

# LocalStack endpoints (SQS, S3)
export SQS_ENDPOINT_URL=http://localhost:4566
export S3_ENDPOINT_URL=http://localhost:4566

# AWS credentials (for LocalStack, use dummy values)
export AWS_ACCESS_KEY_ID=test
export AWS_SECRET_ACCESS_KEY=test
```

## Testing

### Test DynamoDB Local
```bash
# List tables
aws dynamodb list-tables --endpoint-url http://localhost:8000 --region us-east-1

# Create a test table
aws dynamodb create-table \
  --table-name test-table \
  --attribute-definitions AttributeName=id,AttributeType=S \
  --key-schema AttributeName=id,KeyType=HASH \
  --billing-mode PAY_PER_REQUEST \
  --endpoint-url http://localhost:8000 \
  --region us-east-1
```

### Test LocalStack SQS
```bash
# Create a queue
aws sqs create-queue \
  --queue-name test-queue \
  --endpoint-url http://localhost:4566 \
  --region us-east-1

# List queues
aws sqs list-queues \
  --endpoint-url http://localhost:4566 \
  --region us-east-1
```

### Test LocalStack S3
```bash
# Create a bucket
aws s3 mb s3://test-bucket \
  --endpoint-url http://localhost:4566 \
  --region us-east-1

# List buckets
aws s3 ls \
  --endpoint-url http://localhost:4566
```

## Running Tests

With the services running, you can run integration tests:

```bash
# Set environment variables
export DYNAMODB_ENDPOINT_URL=http://localhost:8000
export SQS_ENDPOINT_URL=http://localhost:4566
export S3_ENDPOINT_URL=http://localhost:4566
export AWS_REGION=us-east-1
export AWS_ACCESS_KEY_ID=test
export AWS_SECRET_ACCESS_KEY=test

# Run all AWS integration tests (recommended)
make test-aws-local

# Or run individual tests manually:
# Run tests with DDB backend
cargo test -p plexspaces-locks --features ddb-backend --test ddb_integration
cargo test -p plexspaces-scheduler --features ddb-backend --test ddb_integration
cargo test -p plexspaces-keyvalue --features ddb-backend --test ddb_integration_tests
cargo test -p plexspaces-workflow --features ddb-backend --test ddb_storage_integration_test
cargo test -p plexspaces-journaling --features ddb-backend --test ddb_integration_tests
cargo test -p plexspaces-tuplespace --features ddb-backend --test ddb_storage_integration_tests
cargo test -p plexspaces-blob --features ddb-backend --test ddb_repository_integration_tests
cargo test -p plexspaces-facet --features ddb-backend --test keyvalue_ddb_tests

# Run tests with SQS backend
cargo test -p plexspaces-channel --features sqs-backend --test sqs_integration
```

## Data Persistence

- **DynamoDB Local**: Uses `-inMemory` flag (data lost on restart)
- **LocalStack**: Data persisted in `./localstack-data` volume

## Troubleshooting

### Port Already in Use
If ports 8000 or 4566 are already in use, modify the port mappings in `docker-compose.aws-local.yml`.

### Services Not Starting
Check logs:
```bash
docker-compose -f docker-compose.aws-local.yml logs
```

### Connection Refused
Ensure services are healthy:
```bash
docker-compose -f docker-compose.aws-local.yml ps
```

Wait for services to be healthy before running tests.

