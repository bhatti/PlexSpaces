#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Simple test script for blob APIs

set -e

NODE_URL="${1:-http://localhost:9000}"
TENANT_ID="test-tenant"
NAMESPACE="test-namespace"

echo "Testing Blob APIs at $NODE_URL"
echo ""

# Create test file
TEST_FILE=$(mktemp)
echo "Hello, World! Test file content." > "$TEST_FILE"
echo "Created test file: $TEST_FILE"
echo ""

# Test 1: Upload
echo "1. Uploading blob..."
UPLOAD_OUT=$(curl -s -X POST "${NODE_URL}/api/v1/blobs/upload" \
  -F "file=@${TEST_FILE}" \
  -F "tenant_id=${TENANT_ID}" \
  -F "namespace=${NAMESPACE}" \
  -F "content_type=text/plain" 2>&1)

if echo "$UPLOAD_OUT" | grep -q "blob_id"; then
    BLOB_ID=$(echo "$UPLOAD_OUT" | grep -o '"blob_id":"[^"]*' | cut -d'"' -f4)
    echo "✓ Upload successful: $BLOB_ID"
    echo "$UPLOAD_OUT" | python3 -m json.tool 2>/dev/null || echo "$UPLOAD_OUT"
else
    echo "✗ Upload failed:"
    echo "$UPLOAD_OUT"
    rm -f "$TEST_FILE"
    exit 1
fi
echo ""

# Test 2: Download
if [ -n "$BLOB_ID" ]; then
    echo "2. Downloading blob..."
    DOWNLOAD_OUT=$(curl -s -X GET "${NODE_URL}/api/v1/blobs/${BLOB_ID}/download/raw" 2>&1)
    if [ "$DOWNLOAD_OUT" = "$(cat "$TEST_FILE")" ]; then
        echo "✓ Download successful - content matches"
    else
        echo "✗ Download failed - content mismatch"
        echo "Expected: $(cat "$TEST_FILE")"
        echo "Got: $DOWNLOAD_OUT"
    fi
    echo ""
fi

# Test 3: Get metadata
if [ -n "$BLOB_ID" ]; then
    echo "3. Getting metadata..."
    METADATA_OUT=$(curl -s -X GET "${NODE_URL}/api/v1/blobs/${BLOB_ID}" 2>&1)
    if echo "$METADATA_OUT" | grep -q "blob_id"; then
        echo "✓ Get metadata successful"
        echo "$METADATA_OUT" | python3 -m json.tool 2>/dev/null || echo "$METADATA_OUT"
    else
        echo "✗ Get metadata failed:"
        echo "$METADATA_OUT"
    fi
    echo ""
fi

# Test 4: List blobs
echo "4. Listing blobs..."
LIST_OUT=$(curl -s -X GET "${NODE_URL}/api/v1/blobs?tenant_id=${TENANT_ID}&namespace=${NAMESPACE}" 2>&1)
if echo "$LIST_OUT" | grep -q "blobs"; then
    echo "✓ List blobs successful"
    echo "$LIST_OUT" | python3 -m json.tool 2>/dev/null || echo "$LIST_OUT"
else
    echo "✗ List blobs failed:"
    echo "$LIST_OUT"
fi
echo ""

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
    echo ""
fi

# Cleanup
rm -f "$TEST_FILE"
echo "Tests completed!"
