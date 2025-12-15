#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Test script for blob service HTTP APIs
#
# Usage:
#   ./scripts/test-blob-api.sh [NODE_URL]
#
# Default NODE_URL: http://localhost:9000

set -euo pipefail

NODE_URL="${1:-http://localhost:9000}"
TENANT_ID="test-tenant"
NAMESPACE="test-namespace"

echo "🧪 Testing Blob Service APIs at ${NODE_URL}"
echo ""

# Colors for output
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

# Helper function to print test result
test_result() {
    if [ $? -eq 0 ]; then
        echo -e "${GREEN}✓${NC} $1"
        ((TESTS_PASSED++))
    else
        echo -e "${RED}✗${NC} $1"
        ((TESTS_FAILED++))
    fi
}

# Create a test file
TEST_FILE=$(mktemp)
echo "Hello, World! This is a test file." > "$TEST_FILE"
echo "Created test file: $TEST_FILE"
echo ""

# Test 1: Upload a blob via HTTP
echo "📤 Test 1: Upload blob via HTTP (multipart/form-data)"
UPLOAD_RESPONSE=$(curl -s -w "\n%{http_code}" -X POST "${NODE_URL}/api/v1/blobs/upload" \
    -F "file=@${TEST_FILE}" \
    -F "tenant_id=${TENANT_ID}" \
    -F "namespace=${NAMESPACE}" \
    -F "content_type=text/plain" \
    -F "blob_group=test-group" \
    -F "kind=test-kind")

HTTP_CODE=$(echo "$UPLOAD_RESPONSE" | tail -n1)
BODY=$(echo "$UPLOAD_RESPONSE" | sed '$d')

if [ "$HTTP_CODE" -eq 200 ]; then
    BLOB_ID=$(echo "$BODY" | jq -r '.blob_id // empty')
    if [ -n "$BLOB_ID" ] && [ "$BLOB_ID" != "null" ]; then
        echo "  Blob ID: $BLOB_ID"
        test_result "Upload successful"
        echo "$BODY" | jq '.'
    else
        echo -e "${RED}✗${NC} Upload failed: Invalid response"
        ((TESTS_FAILED++))
        exit 1
    fi
else
    echo -e "${RED}✗${NC} Upload failed: HTTP $HTTP_CODE"
    echo "$BODY"
    ((TESTS_FAILED++))
    exit 1
fi
echo ""

# Test 2: Download blob via HTTP
if [ -n "$BLOB_ID" ] && [ "$BLOB_ID" != "null" ]; then
    echo "📥 Test 2: Download blob via HTTP (raw)"
    DOWNLOAD_RESPONSE=$(curl -s -w "\n%{http_code}" -X GET "${NODE_URL}/api/v1/blobs/${BLOB_ID}/download/raw")
    HTTP_CODE=$(echo "$DOWNLOAD_RESPONSE" | tail -n1)
    BODY=$(echo "$DOWNLOAD_RESPONSE" | sed '$d')
    
    if [ "$HTTP_CODE" -eq 200 ]; then
        if [ "$BODY" = "$(cat "$TEST_FILE")" ]; then
            test_result "Download successful - content matches"
        else
            echo -e "${RED}✗${NC} Download failed: Content mismatch"
            ((TESTS_FAILED++))
        fi
    else
        echo -e "${RED}✗${NC} Download failed: HTTP $HTTP_CODE"
        echo "$BODY"
        ((TESTS_FAILED++))
    fi
    echo ""
fi

# Test 3: Get blob metadata via gRPC-Gateway (HTTP)
if [ -n "$BLOB_ID" ] && [ "$BLOB_ID" != "null" ]; then
    echo "📋 Test 3: Get blob metadata via gRPC-Gateway (HTTP)"
    METADATA_RESPONSE=$(curl -s -w "\n%{http_code}" -X GET "${NODE_URL}/api/v1/blobs/${BLOB_ID}")
    HTTP_CODE=$(echo "$METADATA_RESPONSE" | tail -n1)
    BODY=$(echo "$METADATA_RESPONSE" | sed '$d')
    
    if [ "$HTTP_CODE" -eq 200 ]; then
        METADATA_BLOB_ID=$(echo "$BODY" | jq -r '.metadata.blob_id // empty')
        if [ "$METADATA_BLOB_ID" = "$BLOB_ID" ]; then
            test_result "Get metadata successful"
            echo "$BODY" | jq '.metadata | {blob_id, tenant_id, namespace, name, content_type, content_length}'
        else
            echo -e "${RED}✗${NC} Get metadata failed: Blob ID mismatch"
            ((TESTS_FAILED++))
        fi
    else
        echo -e "${RED}✗${NC} Get metadata failed: HTTP $HTTP_CODE"
        echo "$BODY"
        ((TESTS_FAILED++))
    fi
    echo ""
fi

# Test 4: List blobs via gRPC-Gateway (HTTP)
echo "📋 Test 4: List blobs via gRPC-Gateway (HTTP)"
LIST_RESPONSE=$(curl -s -w "\n%{http_code}" -X GET "${NODE_URL}/api/v1/blobs?tenant_id=${TENANT_ID}&namespace=${NAMESPACE}" 2>&1)
HTTP_CODE=$(echo "$LIST_RESPONSE" | tail -n1)
BODY=$(echo "$LIST_RESPONSE" | sed '$d')

if [ "$HTTP_CODE" -eq 200 ]; then
    BLOB_COUNT=$(echo "$BODY" | jq '.blobs | length')
    if [ "$BLOB_COUNT" -gt 0 ]; then
        test_result "List blobs successful - found $BLOB_COUNT blob(s)"
        echo "$BODY" | jq '.blobs[] | {blob_id, name, content_type, content_length}'
    else
        echo -e "${YELLOW}⚠${NC} List blobs returned empty (might be expected)"
        ((TESTS_PASSED++))
    fi
else
    echo -e "${RED}✗${NC} List blobs failed: HTTP $HTTP_CODE"
    echo "$BODY"
    ((TESTS_FAILED++))
fi
echo ""

# Test 5: Delete blob via gRPC-Gateway (HTTP)
if [ -n "$BLOB_ID" ] && [ "$BLOB_ID" != "null" ]; then
    echo "🗑️  Test 5: Delete blob via gRPC-Gateway (HTTP)"
    DELETE_RESPONSE=$(curl -s -w "\n%{http_code}" -X DELETE "${NODE_URL}/api/v1/blobs/${BLOB_ID}")
    HTTP_CODE=$(echo "$DELETE_RESPONSE" | tail -n1)
    
    if [ "$HTTP_CODE" -eq 200 ]; then
        test_result "Delete successful"
        
        # Verify deletion
        VERIFY_RESPONSE=$(curl -s -w "\n%{http_code}" -X GET "${NODE_URL}/api/v1/blobs/${BLOB_ID}")
        VERIFY_HTTP_CODE=$(echo "$VERIFY_RESPONSE" | tail -n1)
        if [ "$VERIFY_HTTP_CODE" -ne 200 ]; then
            test_result "Verification: Blob deleted (not found as expected)"
        else
            echo -e "${RED}✗${NC} Verification failed: Blob still exists"
            ((TESTS_FAILED++))
        fi
    else
        echo -e "${RED}✗${NC} Delete failed: HTTP $HTTP_CODE"
        ((TESTS_FAILED++))
    fi
    echo ""
fi

# Cleanup
rm -f "$TEST_FILE"

# Summary
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Test Summary:"
echo -e "  ${GREEN}Passed: ${TESTS_PASSED}${NC}"
if [ $TESTS_FAILED -gt 0 ]; then
    echo -e "  ${RED}Failed: ${TESTS_FAILED}${NC}"
    exit 1
else
    echo -e "  ${GREEN}Failed: ${TESTS_FAILED}${NC}"
    exit 0
fi
