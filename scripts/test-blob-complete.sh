#!/bin/bash
# Complete test script for blob service APIs

set -e

NODE_URL="${1:-http://localhost:9002}"
TENANT_ID="test-tenant-$(date +%s)"
NAMESPACE="test-namespace"

echo "🧪 Complete Blob Service API Test"
echo "Node URL: $NODE_URL"
echo ""

# Create test file
TEST_FILE=$(mktemp)
echo "Hello, World! Test file content." > "$TEST_FILE"

# Test 1: Upload
echo "1. Uploading blob..."
UPLOAD_OUT=$(curl -s -X POST "${NODE_URL}/api/v1/blobs/upload" \
  -F "file=@${TEST_FILE}" \
  -F "tenant_id=${TENANT_ID}" \
  -F "namespace=${NAMESPACE}" \
  -F "content_type=text/plain" 2>&1)

if echo "$UPLOAD_OUT" | grep -q "blob_id"; then
    BLOB_ID=$(echo "$UPLOAD_OUT" | python3 -c "import sys, json; print(json.load(sys.stdin).get('blob_id', ''))" 2>/dev/null || echo "")
    echo "✓ Upload successful: $BLOB_ID"
else
    echo "✗ Upload failed:"
    echo "$UPLOAD_OUT"
    rm -f "$TEST_FILE"
    exit 1
fi

# Test 2: Download
if [ -n "$BLOB_ID" ]; then
    echo "2. Downloading blob..."
    DOWNLOAD_OUT=$(curl -s -X GET "${NODE_URL}/api/v1/blobs/${BLOB_ID}/download/raw" 2>&1)
    if [ "$DOWNLOAD_OUT" = "$(cat "$TEST_FILE")" ]; then
        echo "✓ Download successful"
    else
        echo "✗ Download failed"
    fi
fi

# Test 3: Get metadata
if [ -n "$BLOB_ID" ]; then
    echo "3. Getting metadata..."
    METADATA_OUT=$(curl -s -X GET "${NODE_URL}/api/v1/blobs/${BLOB_ID}" 2>&1)
    if echo "$METADATA_OUT" | grep -q "blob_id"; then
        echo "✓ Get metadata successful"
    else
        echo "✗ Get metadata failed"
    fi
fi

# Test 4: List
echo "4. Listing blobs..."
LIST_OUT=$(curl -s -X GET "${NODE_URL}/api/v1/blobs?tenant_id=${TENANT_ID}&namespace=${NAMESPACE}" 2>&1)
if echo "$LIST_OUT" | grep -q "blobs"; then
    echo "✓ List blobs successful"
else
    echo "✗ List blobs failed"
fi

# Test 5: Delete
if [ -n "$BLOB_ID" ]; then
    echo "5. Deleting blob..."
    DELETE_OUT=$(curl -s -w "\n%{http_code}" -X DELETE "${NODE_URL}/api/v1/blobs/${BLOB_ID}" 2>&1)
    HTTP_CODE=$(echo "$DELETE_OUT" | tail -n1)
    if [ "$HTTP_CODE" = "200" ]; then
        echo "✓ Delete successful"
    else
        echo "✗ Delete failed: HTTP $HTTP_CODE"
    fi
fi

rm -f "$TEST_FILE"
echo "Tests completed!"
