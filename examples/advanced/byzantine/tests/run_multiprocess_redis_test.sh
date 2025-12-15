#!/bin/bash
# Multi-process Byzantine Generals test with shared Redis
#
# This script demonstrates PlexSpaces multi-process coordination using
# shared Redis TupleSpace backend.

set -e

echo "🚀 Starting Multi-Process Byzantine Generals Test with Shared Redis"
echo "===================================================================="
echo ""

# Check if Redis is running
echo "📍 Checking Redis availability..."
if redis-cli ping > /dev/null 2>&1; then
    echo "✅ Redis is running on localhost:6379"
else
    echo "❌ Redis not running! Starting Redis in Docker..."
    docker run -d --rm -p 6379:6379 --name byzantine-redis redis:7
    echo "⏳ Waiting for Redis to start..."
    sleep 2

    if redis-cli ping > /dev/null 2>&1; then
        echo "✅ Redis started successfully"
    else
        echo "❌ Failed to start Redis. Please start it manually:"
        echo "   docker run -p 6379:6379 redis:7"
        exit 1
    fi
fi

echo ""

# Clean up any existing tuples from previous runs
echo "🧹 Cleaning up Redis (flushing byzantine:* keys)..."
redis-cli --scan --pattern "byzantine:*" | xargs -r redis-cli DEL
echo "✅ Redis cleaned"

echo ""

# Build Byzantine node binary with Redis backend
echo "🔨 Building Byzantine node binary..."
cd "$(dirname "$0")/.."
cargo build --bin byzantine_node --features redis-backend --quiet
if [ $? -ne 0 ]; then
    echo "❌ Build failed!"
    exit 1
fi
echo "✅ Build complete"

echo ""

# Start Node 1 (2 generals: general0 commander, general1)
echo "📍 Starting Node 1 (general0, general1)..."
./target/debug/byzantine_node \
  --node-id node1 \
  --address localhost:19001 \
  --tuplespace-backend redis \
  --redis-url redis://localhost:6379 \
  --generals general0,general1 &
NODE1_PID=$!
echo "   Process ID: $NODE1_PID"

sleep 3

# Start Node 2 (2 generals: general2, general3)
echo "📍 Starting Node 2 (general2, general3)..."
./target/debug/byzantine_node \
  --node-id node2 \
  --address localhost:19002 \
  --tuplespace-backend redis \
  --redis-url redis://localhost:6379 \
  --generals general2,general3 &
NODE2_PID=$!
echo "   Process ID: $NODE2_PID"

echo ""
echo "⏳ Waiting for consensus (30 seconds)..."
echo "   (Generals are coordinating via shared Redis TupleSpace)"
sleep 30

echo ""

# Check results in Redis
echo "🔍 Checking votes in shared Redis..."
echo "===================================="
echo "Keys in Redis:"
redis-cli KEYS "byzantine:*" || true

echo ""
echo "Vote tuples:"
redis-cli --scan --pattern "byzantine:*" | while read key; do
    echo "  $key:"
    redis-cli GET "$key" || redis-cli HGETALL "$key" || redis-cli SMEMBERS "$key" || true
done

echo ""

# Cleanup processes
echo "🧹 Cleaning up processes..."
kill $NODE1_PID $NODE2_PID 2>/dev/null || true
wait $NODE1_PID $NODE2_PID 2>/dev/null || true

# Stop Redis if we started it
if docker ps | grep -q byzantine-redis; then
    echo "🛑 Stopping Redis container..."
    docker stop byzantine-redis 2>/dev/null || true
fi

echo ""
echo "✅ Multi-process test complete!"
echo ""
echo "Summary:"
echo "  - 2 processes (nodes) started"
echo "  - 4 generals total (2 per node)"
echo "  - All coordinated via shared Redis TupleSpace"
echo "  - Check output above for consensus results"
