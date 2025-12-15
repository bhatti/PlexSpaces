#!/bin/bash
# Archive old planning documents that have been consolidated into PROJECT_TRACKER.md
# Created: 2025-11-10

set -e  # Exit on error

# Create archive directory
ARCHIVE_DIR="archive/planning_docs_2025-11-10"
mkdir -p "$ARCHIVE_DIR"

echo "📦 Archiving planning documents to $ARCHIVE_DIR..."
echo ""

# List of files to archive (14 files consolidated into PROJECT_TRACKER.md)
FILES_TO_ARCHIVE=(
    "COHESIVE_DESIGN_V2.md"
    "COHESIVE_DESIGN.md"
    "DEVELOPMENT_TRACKER.md"
    "FEATURE_CHECKLIST.md"
    "FEATURE_CONSOLIDATION.md"
    "GAP_ANALYSIS.md"
    "MIGRATION_PHASE1_COMPLETE.md"
    "MIGRATION_PROGRESS.md"
    "MIGRATION_STRATEGY.md"
    "MIGRATION_TRACKER.md"
    "PHASE1_COMPLETE.md"
    "PROTO_FIRST_AUDIT.md"
    "WALKING_SKELETON_PLAN.md"
    "WALKING_SKELETON_STATUS.md"
)

# Archive each file
for file in "${FILES_TO_ARCHIVE[@]}"; do
    if [ -f "$file" ]; then
        echo "✅ Archiving $file"
        mv "$file" "$ARCHIVE_DIR/"
    else
        echo "⚠️  $file not found (skipping)"
    fi
done

echo ""
echo "✅ Archive complete!"
echo ""
echo "📄 Files archived: ${#FILES_TO_ARCHIVE[@]}"
echo "📂 Location: $ARCHIVE_DIR"
echo ""
echo "Active planning documents remaining:"
echo "  📄 PROJECT_TRACKER.md (MASTER - use this!)"
echo "  📄 CLAUDE.md (main instructions)"
echo "  📄 IMPLEMENTATION_ROADMAP.md (optional - day-by-day plan)"
echo "  📄 LOW_PRIORITY_FEATURES.md (deferred features)"
echo "  📄 GRPC_MIDDLEWARE_DESIGN.md (gRPC middleware spec)"
echo "  📄 KEYVALUE_USE_CASES.md (KeyValue design)"
echo "  📄 REGISTRY_STORAGE_ANALYSIS.md (design decision)"
echo "  📄 EXAMPLES_STATUS.md (example config status)"
echo ""
echo "🎯 Next: Use PROJECT_TRACKER.md as your single source of truth!"
