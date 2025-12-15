#!/bin/bash

set -euo pipefail

echo "🚀 Setting up PlexSpaces Framework development environment"

# Check if we're in the right directory
if [[ ! -f "Cargo.toml" ]]; then
    echo "❌ Error: Cargo.toml not found. Please run this script from the project root."
    exit 1
fi

# Make scripts executable
chmod +x scripts/*.sh

# Install dependencies using Makefile
echo "📦 Installing dependencies..."
make deps

# Generate initial code
echo "🔄 Generating initial code..."
make generate

# Build the project
echo "🔨 Building project..."
make build

# Run tests to make sure everything works
echo "🧪 Running tests..."
make test

echo "✅ Setup completed successfully!"
echo ""
echo "🎯 Next steps:"
echo "  • Run 'make dev' to start development mode with file watching"
echo "  • Run 'make help' to see all available commands"
echo "  • Check docs/ directory for generated documentation"

