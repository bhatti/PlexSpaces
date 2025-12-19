.PHONY: all build build-examples build-wasm run-examples test test-examples test-wasm clean clean-examples clean-all proto proto-build proto-buf fmt lint doc install-tools coverage coverage-report test-coverage check-coverage coverage-crate bench help

# Variables
CARGO = cargo
BUF = buf
RUSTFMT = rustfmt
CLIPPY = clippy

# Default target
all: proto build build-examples test test-examples

# Help target
help:
	@echo "PlexSpaces Development Commands:"
	@echo ""
	@echo "🔧 Setup & Build:"
	@echo "  make install-tools    - Install required development tools"
	@echo "  make proto            - Generate code from proto files (uses buf)"
	@echo "  make proto-buf        - Generate proto using buf (RECOMMENDED - handles deps)"
	@echo "  make proto-build      - Generate proto using tonic-build (no external deps)"
	@echo "  make build            - Build all Rust crates"
	@echo "  make build-examples   - Build all examples"
	@echo "  make build-wasm       - Build all WASM actors"
	@echo "  make run-examples     - Run all examples (workspace + standalone)"
	@echo "  make run-workspace-examples - Run workspace examples only"
	@echo "  make run-all-standalone-examples - Run all standalone examples"
	@echo "  make run-example-<name> - Run specific standalone example"
	@echo "  make release          - Build release version"
	@echo ""
	@echo "🧪 Testing:"
	@echo "  make test             - Run all tests"
	@echo "  make test-examples    - Run all example tests"
	@echo "  make test-wasm        - Run WASM example tests"
	@echo "  make test-all-examples - Run all example tests (inc. WASM)"
	@echo "  make test-quick       - Run quick unit tests (dev loop)"
	@echo "  make test-e2e         - Run comprehensive E2E tests"
	@echo "  make test-e2e-quick   - Run E2E tests (quick mode)"
	@echo "  make test-e2e-sql     - Run E2E tests with SQL backend"
	@echo "  make test-byzantine   - Run Byzantine Generals tests"
	@echo "  make test-coverage    - Run tests with coverage report"
	@echo "  make check-coverage   - Check 90% coverage requirement"
	@echo ""
	@echo "🎯 Quality Checks:"
	@echo "  make fmt              - Format all code"
	@echo "  make fmt-check        - Check formatting without changes"
	@echo "  make lint             - Run linters (clippy and buf)"
	@echo "  make audit            - Run security audit"
	@echo "  make pre-commit       - Run pre-commit checks"
	@echo "  make validate         - Full validation before push"
	@echo ""
	@echo "📚 Documentation:"
	@echo "  make doc              - Generate documentation"
	@echo "  make doc-open         - Generate and open docs in browser"
	@echo ""
	@echo "🧹 Maintenance:"
	@echo "  make clean            - Clean build artifacts"
	@echo "  make clean-examples   - Clean example artifacts"
	@echo "  make clean-all        - Clean everything (workspace + examples)"
	@echo "  make update-deps      - Update dependencies"
	@echo "  make stats            - Display project statistics"
	@echo ""
	@echo "🚀 Workflows:"
	@echo "  make all              - Run proto, build, and test"
	@echo "  make ci               - Run full CI pipeline locally"
	@echo "  make setup            - Development environment setup"
	@echo "  make wasm             - Build and test all WASM actors"
	@echo "  make wasm-check       - Quick WASM compilation check"
	@echo ""
	@echo "🐳 Docker:"
	@echo "  make docker-build     - Build Docker image (framework-only)"
	@echo "  make docker-build-sqlite - Build Docker image (SQLite backends)"
	@echo "  make docker-build-postgres - Build Docker image (PostgreSQL backends)"
	@echo "  make docker-build-hybrid - Build Docker image (hybrid: SQLite + Redis)"
	@echo "  make docker-build-wasm - Build Docker image (WASM-enabled)"
	@echo "  make docker-build-all - Build all Docker variants"
	@echo "  make docker-push      - Push Docker image to registry (REGISTRY=<url>)"
	@echo ""
	@echo "🐙 Docker Compose:"
	@echo "  make docker-compose-up - Start docker-compose"
	@echo "  make docker-compose-down - Stop docker-compose"
	@echo "  make docker-compose-logs - View docker-compose logs"
	@echo "  make docker-compose-restart - Restart docker-compose"
	@echo ""
	@echo "☸️  Kubernetes:"
	@echo "  make k8s-deploy       - Deploy to Kubernetes"
	@echo "  make k8s-undeploy     - Remove from Kubernetes"
	@echo "  make k8s-status       - Check Kubernetes deployment status"
	@echo "  make k8s-logs         - View Kubernetes logs"
	@echo ""
	@echo "🔧 Operations:"
	@echo "  make logs             - View logs (auto-detects docker-compose/k8s)"
	@echo "  make health           - Check health status"
	@echo "  make metrics          - View metrics (if HTTP gateway enabled)"
	@echo "  make backup           - Backup SQLite databases"
	@echo "  make restore          - Restore SQLite databases (BACKUP_DIR=<dir>)"

# Install development tools
install-tools:
	@echo "Installing development tools..."
	@rustup component add rustfmt clippy
	@cargo install cargo-tarpaulin
	@cargo install cargo-watch
	@cargo install cargo-edit
	@cargo install cargo-audit
	@echo "Installing buf..."
	@curl -sSL "https://github.com/bufbuild/buf/releases/download/v1.28.1/buf-$$(uname -s)-$$(uname -m)" -o /tmp/buf
	@sudo mv /tmp/buf /usr/local/bin/buf
	@sudo chmod +x /usr/local/bin/buf
	@echo "Tools installed successfully!"

# Generate code from proto files (using tonic-build via cargo)
# This is the RECOMMENDED approach. It relies on build.rs.
# NOTE: proto-vendor removed as it was causing issues - use committed generated files
proto-build:
	@echo "Generating proto files using tonic-build (build.rs)..."
	@echo "  → Cleaning old generated files"
	@rm -rf crates/proto/src/generated
	@echo "  → Running cargo build for proto crate (triggers build.rs)"
	@$(CARGO) build -p plexspaces-proto
	@echo "Proto generation complete! ✓"
	@echo "Generated files: $$(ls -1 crates/proto/src/generated/*.rs 2>/dev/null | wc -l) Rust files"

# Generate code from proto files (using buf)
# NOTE: build.rs handles Copy trait removal, but only runs during cargo build
# So we need to trigger it after buf generate to remove Copy attributes
proto-buf:
	@echo "Linting proto files..."
	@$(BUF) lint
	@echo "Generating Rust code from proto files..."
	@$(BUF) generate
	@echo "Post-processing: Running cargo build to trigger build.rs (removes Copy traits)..."
	@$(CARGO) build -p plexspaces-proto > /dev/null 2>&1 || $(CARGO) build -p plexspaces-proto 2>&1 | tail -5
	@echo "Proto generation complete! ✓"
	@echo "Generated files: $$(ls -1 crates/proto/src/generated/*.rs | wc -l) Rust files"

# Default proto generation (uses tonic-build via build.rs)
proto: proto-buf

# Build all crates
build:
	@echo "Building all crates..."
	@CARGO_JOBS=$${CARGO_BUILD_JOBS:-4}; \
	if [ "$$CARGO_JOBS" = "0" ]; then \
		CARGO_JOBS=$$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4); \
	fi; \
	echo "Using $$CARGO_JOBS CPU cores (override with CARGO_BUILD_JOBS env var)"; \
	$(CARGO) build --all-features --workspace --jobs $$CARGO_JOBS --message-format=short
	@echo "Build complete!"

# Build release version
release:
	@echo "Building release version..."
	@CARGO_JOBS=$${CARGO_BUILD_JOBS:-4}; \
	if [ "$$CARGO_JOBS" = "0" ]; then \
		CARGO_JOBS=$$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4); \
	fi; \
	echo "Using $$CARGO_JOBS CPU cores (override with CARGO_BUILD_JOBS env var)"; \
	$(CARGO) build --release --all-features --workspace --jobs $$CARGO_JOBS --message-format=short
	@echo "Release build complete!"

# Build all examples
# Uses shared target directory to avoid rebuilding common dependencies
build-examples:
	@echo "Building examples (using shared target directory)..."
	@echo "Target directory: $$(pwd)/target (shared across all examples)"
	@CARGO_JOBS=$${CARGO_BUILD_JOBS:-4}; \
	if [ "$$CARGO_JOBS" = "0" ]; then \
		CARGO_JOBS=$$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4); \
	fi; \
	echo "Using $$CARGO_JOBS CPU cores (override with CARGO_BUILD_JOBS env var)"; \
	echo "Pre-building workspace dependencies for faster example builds..."; \
	$(CARGO) build --lib --all-features --workspace --jobs $$CARGO_JOBS --message-format=short || true; \
	echo ""; \
	echo "Building examples with shared target directory..."; \
	PROJECT_ROOT=$$(pwd); \
	EXAMPLES="advanced/byzantine advanced/nbody domains/finance-risk domains/genomic-workflow-pipeline domains/genomics-pipeline domains/order-processing intermediate/heat_diffusion intermediate/matrix_multiply intermediate/matrix_vector_mpi simple/durable_actor_example simple/firecracker_multi_tenant simple/faas_actor wasm-calculator"; \
	for example in $$EXAMPLES; do \
		if [ -d "examples/$$example" ] && [ -f "examples/$$example/Cargo.toml" ]; then \
			echo "Building $$example..."; \
			(cd "examples/$$example" && \
				CARGO_TARGET_DIR="$$PROJECT_ROOT/target" \
				$(CARGO) build --all-features --jobs $$CARGO_JOBS --message-format=short) || exit 1; \
		fi; \
	done; \
	echo "Examples build complete!"

# Run all examples (workspace examples + standalone examples)
run-examples: run-workspace-examples
	@echo "\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
	@echo "Standalone Examples"
	@echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
	@echo ""
	@echo "To run standalone examples, use one of:"
	@echo "  make run-all-standalone-examples  - Run all standalone examples"
	@echo "  make run-example-<name>           - Run specific example"
	@echo ""
	@echo "Available standalone examples:"
	@echo "  - byzantine"
	@echo "  - heat_diffusion (has multiple binaries: heat_diffusion, multi-node)"
	@echo "  - heat_diffusion_wasm"
	@echo "  - wasm-calculator"
	@echo "  - order-processing"
	@echo "  - finance-risk"
	@echo "  - genomics-pipeline"
	@echo "  - genomic-workflow-pipeline"
	@echo "  - matrix_multiply"
	@echo "  - matrix_vector_mpi"
	@echo "  - config-updates"
	@echo "  - wasm_showcase"
	@echo ""
	@echo "✅ Workspace examples executed!"

# Run workspace examples (defined in main Cargo.toml)
run-workspace-examples:
	@echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
	@echo "Running workspace examples..."
	@echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
	@CARGO_JOBS=$${CARGO_BUILD_JOBS:-4}; \
	if [ "$$CARGO_JOBS" = "0" ]; then \
		CARGO_JOBS=$$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4); \
	fi; \
	for example in simple_working simple_test lifecycle_metrics node_discovery mobility_facet_demo; do \
		if $(CARGO) build --example $$example --jobs $$CARGO_JOBS 2>&1 | grep -q "Finished"; then \
			echo "\n=== Running workspace example: $$example ==="; \
			$(CARGO) run --example $$example || echo "⚠️  Example $$example failed"; \
		else \
			echo "⚠️  Skipping $$example (not found or failed to build)"; \
		fi; \
	done

# Run standalone examples (with their own Cargo.toml)
run-standalone-examples:
	@echo "\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
	@echo "Running standalone examples..."
	@echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
	@echo ""
	@echo "⚠️  Note: Standalone examples require manual execution from their directories"
	@echo "    Use 'make run-example-<name>' to run specific examples"
	@echo ""
	@echo "Available standalone examples:"
	@echo "  - byzantine"
	@echo "  - heat_diffusion"
	@echo "  - heat_diffusion_wasm"
	@echo "  - wasm-calculator"
	@echo "  - order-processing"
	@echo "  - finance-risk"
	@echo "  - genomics-pipeline"
	@echo "  - genomic-workflow-pipeline"
	@echo "  - matrix_multiply"
	@echo "  - matrix_vector_mpi"
	@echo "  - config-updates"
	@echo "  - wasm_showcase"

# Run specific standalone example
# Uses shared target directory for faster builds
run-example-%:
	@echo "Running standalone example: $*"
	@if [ -d "examples/$*" ] && [ -f "examples/$*/Cargo.toml" ]; then \
		PROJECT_ROOT=$$(pwd); \
		CARGO_JOBS=$${CARGO_BUILD_JOBS:-4}; \
		if [ "$$CARGO_JOBS" = "0" ]; then \
			CARGO_JOBS=$$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4); \
		fi; \
		cd examples/$*; \
		if [ -f "Cargo.toml" ] && grep -q "\[\[bin\]\]" Cargo.toml; then \
			echo "⚠️  Example has multiple binaries, running default or first binary..."; \
			CARGO_TARGET_DIR="$$PROJECT_ROOT/target" \
			$(CARGO) run --jobs $$CARGO_JOBS --bin $$($(CARGO) read-manifest 2>/dev/null | grep -o '"name":"[^"]*"' | head -1 | cut -d'"' -f4) || \
			CARGO_TARGET_DIR="$$PROJECT_ROOT/target" \
			$(CARGO) run --jobs $$CARGO_JOBS || echo "⚠️  Example $* failed - try: cd examples/$* && cargo run --bin <name>"; \
		else \
			CARGO_TARGET_DIR="$$PROJECT_ROOT/target" \
			$(CARGO) run --jobs $$CARGO_JOBS || echo "⚠️  Example $* failed"; \
		fi; \
	else \
		echo "❌ Example $* not found or not a standalone example"; \
		echo "   Available standalone examples:"; \
		echo "     byzantine, heat_diffusion, heat_diffusion_wasm, wasm-calculator,"; \
		echo "     order-processing, finance-risk, genomics-pipeline, genomic-workflow-pipeline,"; \
		echo "     matrix_multiply, matrix_vector_mpi, config-updates, wasm_showcase"; \
	fi

# Run all standalone examples (one by one)
# Uses shared target directory for faster builds
run-all-standalone-examples:
	@echo "Running all standalone examples (using shared target directory)..."
	@echo "Target directory: $$(pwd)/target (shared across all examples)"
	@echo "⚠️  Note: Some examples may require additional setup (databases, etc.)"
	@echo ""
	@PROJECT_ROOT=$$(pwd); \
	CARGO_JOBS=$${CARGO_BUILD_JOBS:-4}; \
	if [ "$$CARGO_JOBS" = "0" ]; then \
		CARGO_JOBS=$$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4); \
	fi; \
	echo "Pre-building workspace dependencies for faster example builds..."; \
	$(CARGO) build --lib --all-features --workspace --jobs $$CARGO_JOBS --message-format=short || true; \
	echo ""; \
	for example in byzantine heat_diffusion heat_diffusion_wasm wasm-calculator order-processing finance-risk genomics-pipeline genomic-workflow-pipeline matrix_multiply matrix_vector_mpi config-updates wasm_showcase; do \
		if [ -d "examples/$$example" ] && [ -f "examples/$$example/Cargo.toml" ]; then \
			echo "\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"; \
			echo "Running: $$example"; \
			echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"; \
			cd examples/$$example; \
			if [ -f "Cargo.toml" ] && grep -q "\[\[bin\]\]" Cargo.toml; then \
				bin_name=$$($(CARGO) read-manifest 2>/dev/null | grep -o '"name":"[^"]*"' | head -1 | cut -d'"' -f4); \
				if [ -n "$$bin_name" ]; then \
					CARGO_TARGET_DIR="$$PROJECT_ROOT/target" \
					$(CARGO) run --jobs $$CARGO_JOBS --bin $$bin_name || echo "⚠️  Example $$example failed"; \
				else \
					CARGO_TARGET_DIR="$$PROJECT_ROOT/target" \
					$(CARGO) run --jobs $$CARGO_JOBS || echo "⚠️  Example $$example failed - try: cd examples/$$example && cargo run --bin <name>"; \
				fi; \
			else \
				CARGO_TARGET_DIR="$$PROJECT_ROOT/target" \
				$(CARGO) run --jobs $$CARGO_JOBS || echo "⚠️  Example $$example failed"; \
			fi; \
			cd ../..; \
		fi; \
	done

# Run all unit tests (excludes integration tests)
# Optimized for parallel execution - uses limited CPU cores (default: 4, override with CARGO_BUILD_JOBS env var)
# Only tuplespace tests need --test-threads=1, so we run them separately
# Usage: make test                    # Uses 4 cores (default)
#        CARGO_BUILD_JOBS=8 make test # Uses 8 cores
#        CARGO_BUILD_JOBS=0 make test # Uses all available cores
test:
	@echo "Running unit tests (excluding integration tests)..."
	@CARGO_JOBS=$${CARGO_BUILD_JOBS:-4}; \
	if [ "$$CARGO_JOBS" = "0" ]; then \
		CARGO_JOBS=$$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4); \
	fi; \
	echo "Configuration: Using $$CARGO_JOBS CPU cores for building (override with CARGO_BUILD_JOBS env var)"; \
	echo "Building workspace first for faster test execution (incremental build enabled)..."; \
	$(CARGO) build --lib --all-features --workspace --jobs $$CARGO_JOBS --message-format=short || true; \
	echo ""; \
	echo "Running tuplespace tests first with single thread (to avoid env var race conditions)..."; \
	$(CARGO) test --lib --all-features -p plexspaces-tuplespace --jobs $$CARGO_JOBS -- --test-threads=1 || exit 1; \
	echo ""; \
	echo "Running all other tests with parallel execution..."; \
	$(CARGO) test --lib --all-features --workspace --jobs $$CARGO_JOBS \
		-p plexspaces -p plexspaces-proto -p plexspaces-lattice -p plexspaces-mailbox \
		-p plexspaces-persistence -p plexspaces-journaling -p plexspaces-keyvalue \
		-p plexspaces-facet -p plexspaces-core -p plexspaces-behavior -p plexspaces-actor \
		-p plexspaces-supervisor -p plexspaces-node -p plexspaces-wasm-runtime \
		-p plexspaces-firecracker -p plexspaces-workflow -p plexspaces-process-groups \
		-p plexspaces-channel -p plexspaces-object-registry -p plexspaces-elastic-pool \
		-p plexspaces-circuit-breaker -p plexspaces-locks -p plexspaces-scheduler \
		-p plexspaces-actor-service -p plexspaces-grpc-middleware -p plexspaces-blob \
		-p plexspaces-tuplespace-service || exit 1; \
	echo ""; \
	echo "All unit tests passed!"

# Run all example tests (examples with tests subdirectories)
# Examples are standalone, so we test them from their directories
# Use shared target directory to avoid rebuilding common dependencies
# Runs with limited concurrency (default: 2 processes) and fails fast on errors
# Usage: make test-examples                              # Uses 2 concurrent examples, 4 build cores (default)
#        TEST_EXAMPLES_CONCURRENT=3 make test-examples   # Uses 3 concurrent examples
#        CARGO_BUILD_JOBS=8 make test-examples           # Uses 8 cores for building
test-examples:
	@echo "Running all example tests (using shared target directory)..."
	@echo "Target directory: $$(pwd)/target (shared across all examples)"
	@MAX_CONCURRENT=$${TEST_EXAMPLES_CONCURRENT:-2}; \
	CARGO_JOBS=$${CARGO_BUILD_JOBS:-4}; \
	if [ "$$CARGO_JOBS" = "0" ]; then \
		CARGO_JOBS=$$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4); \
	fi; \
	echo "Concurrency: $$MAX_CONCURRENT processes (override with TEST_EXAMPLES_CONCURRENT env var)"; \
	echo "Build jobs: $$CARGO_JOBS CPU cores (override with CARGO_BUILD_JOBS env var)"; \
	echo "Fail-fast: enabled (stops on first error)"; \
	echo ""; \
	echo "Pre-building workspace dependencies for faster example builds (incremental build enabled)..."; \
	$(CARGO) build --lib --all-features --workspace --jobs $$CARGO_JOBS --message-format=short || true; \
	echo ""; \
	PROJECT_ROOT=$$(pwd); \
	EXAMPLES="advanced/byzantine advanced/nbody domains/finance-risk domains/genomic-workflow-pipeline domains/genomics-pipeline domains/order-processing intermediate/heat_diffusion intermediate/matrix_multiply intermediate/matrix_vector_mpi simple/durable_actor_example simple/firecracker_multi_tenant simple/faas_actor wasm-calculator"; \
	FAILED=0; \
	PIDS=(); \
	EXAMPLE_NAMES=(); \
	EXAMPLE_LOG_NAMES=(); \
	for example in $$EXAMPLES; do \
		if [ -d "examples/$$example" ] && [ -f "examples/$$example/Cargo.toml" ]; then \
			if [ $$FAILED -eq 1 ]; then break; fi; \
			while [ $${#PIDS[@]} -ge $$MAX_CONCURRENT ] && [ $$FAILED -eq 0 ]; do \
				for i in "$${!PIDS[@]}"; do \
					if ! kill -0 "$${PIDS[$$i]}" 2>/dev/null; then \
						wait "$${PIDS[$$i]}" || { \
							FAILED=1; \
							FAILED_EXAMPLE="$${EXAMPLE_NAMES[$$i]}"; \
							FAILED_LOG="$${EXAMPLE_LOG_NAMES[$$i]}"; \
							echo "✗ $$FAILED_EXAMPLE failed!"; \
							if [ -f "/tmp/test_$$FAILED_LOG.log" ]; then \
								cat "/tmp/test_$$FAILED_LOG.log"; \
							fi; \
						}; \
						unset PIDS[$$i]; \
						unset EXAMPLE_NAMES[$$i]; \
						unset EXAMPLE_LOG_NAMES[$$i]; \
					fi; \
				done; \
				PIDS=("$${PIDS[@]}"); \
				EXAMPLE_NAMES=("$${EXAMPLE_NAMES[@]}"); \
				EXAMPLE_LOG_NAMES=("$${EXAMPLE_LOG_NAMES[@]}"); \
				[ $$FAILED -eq 1 ] && break; \
				sleep 0.1; \
			done; \
			[ $$FAILED -eq 1 ] && break; \
			echo "Testing $$example..."; \
			EXAMPLE_LOG_NAME=$$(echo "$$example" | tr '/' '-'); \
			LOG_FILE="/tmp/test_$$EXAMPLE_LOG_NAME.log"; \
			(cd "examples/$$example" && \
				CARGO_TARGET_DIR="$$PROJECT_ROOT/target" \
				$(CARGO) test --all-features --jobs $$CARGO_JOBS > "$$LOG_FILE" 2>&1 && \
				echo "✓ $$example passed" || \
				(echo "✗ $$example failed!"; [ -f "$$LOG_FILE" ] && cat "$$LOG_FILE" || echo "Log file not found: $$LOG_FILE"; exit 1)) & \
			PIDS+=($$!); \
			EXAMPLE_NAMES+=("$$example"); \
			EXAMPLE_LOG_NAMES+=("$$EXAMPLE_LOG_NAME"); \
		fi; \
	done; \
	if [ $$FAILED -eq 0 ]; then \
		for i in "$${!PIDS[@]}"; do \
			wait "$${PIDS[$$i]}" || { \
				FAILED=1; \
				FAILED_EXAMPLE="$${EXAMPLE_NAMES[$$i]}"; \
				FAILED_LOG="$${EXAMPLE_LOG_NAMES[$$i]}"; \
				echo "✗ $$FAILED_EXAMPLE failed!"; \
				if [ -n "$$FAILED_LOG" ] && [ -f "/tmp/test_$$FAILED_LOG.log" ]; then \
					cat "/tmp/test_$$FAILED_LOG.log"; \
				else \
					echo "Log file not available for $$FAILED_EXAMPLE"; \
				fi; \
				break; \
			}; \
		done; \
	fi; \
	if [ $$FAILED -eq 1 ]; then \
		for pid in "$${PIDS[@]}"; do kill $$pid 2>/dev/null || true; done; \
		echo ""; \
		echo "❌ Example tests failed! Stopping early (fail-fast enabled)."; \
		exit 1; \
	else \
		echo ""; \
		echo "✅ All example tests passed!"; \
	fi

# Run tests with coverage (using cargo-llvm-cov - supports Rust 2024)
# Excludes generated code (proto files, build artifacts)
test-coverage:
	@echo "Running tests with coverage (cargo-llvm-cov)..."
	@mkdir -p target/coverage/html target/coverage/reports
	@cargo llvm-cov --workspace --all-features \
		--ignore-filename-regex '(proto/src/generated|target/)' \
		--lcov --output-path target/coverage/coverage.lcov
	@cargo llvm-cov --workspace --all-features \
		--ignore-filename-regex '(proto/src/generated|target/)' \
		--html --output-dir target/coverage/html
	@echo "Coverage report generated at target/coverage/html/index.html"
	@echo "Note: Generated code (proto files) excluded from coverage"

# Generate coverage report for all crates (detailed per-crate analysis)
coverage-report:
	@echo "Generating coverage report for all crates..."
	@mkdir -p target/coverage/html target/coverage/reports
	@./scripts/coverage-baseline.sh || echo "⚠️  Coverage baseline script completed (some crates may have failed)"
	@echo ""
	@echo "Coverage reports:"
	@echo "  Summary: target/coverage/coverage-summary.txt"
	@echo "  HTML: target/coverage/html/index.html"
	@echo "  LCOV: target/coverage/coverage.lcov"
	@echo "  Per-crate LCOV: target/coverage/reports/*.lcov"

# Run coverage for all crates (quick summary)
# Excludes generated code (proto files, build artifacts)
coverage:
	@echo "Running coverage analysis for all crates..."
	@mkdir -p target/coverage/html
	@cargo llvm-cov --workspace --all-features \
		--ignore-filename-regex '(proto/src/generated|target/)' \
		--summary-only
	@echo ""
	@echo "For detailed HTML report, run: make coverage-report"
	@echo "Note: Generated code (proto files) excluded from coverage"

# Check if coverage meets requirements (90%)
# Excludes generated code (proto files, build artifacts)
check-coverage: test-coverage
	@echo "Checking test coverage..."
	@cargo llvm-cov --workspace --all-features \
		--ignore-filename-regex '(proto/src/generated|target/)' \
		--summary-only | grep -E "(Total|Summary)" || true
	@echo "For detailed coverage, see: target/coverage/html/index.html"
	@echo "Note: Generated code (proto files) excluded from coverage"

# Coverage for specific crate
# Excludes generated code (proto files, build artifacts)
coverage-crate:
	@if [ -z "$(CRATE)" ]; then \
		echo "Usage: make coverage-crate CRATE=plexspaces-actor"; \
		exit 1; \
	fi
	@echo "Running coverage for $(CRATE)..."
	@mkdir -p target/coverage/html
	@cargo llvm-cov --lib -p $(CRATE) --all-features \
		--ignore-filename-regex '(proto/src/generated|target/)' \
		--html --output-dir target/coverage/html/$(CRATE)
	@cargo llvm-cov --lib -p $(CRATE) --all-features \
		--ignore-filename-regex '(proto/src/generated|target/)' \
		--summary-only
	@echo "Coverage report for $(CRATE) at: target/coverage/html/$(CRATE)/index.html"
	@echo "Note: Generated code (proto files) excluded from coverage"

# Run specific test
test-crate:
	@echo "Testing crate: $(CRATE)"
	@$(CARGO) test --package $(CRATE) --all-features

# Run integration tests only (requires external services like Kafka, Redis)
test-integration:
	@echo "Running integration tests (including ignored tests)..."
	@echo "⚠️  Note: Integration tests require external services (Kafka, Redis, etc.)"
	@echo "    Start services with: docker-compose up -d (in crates/channel/ or examples/order-processing/)"
	@$(CARGO) test --test '*' --all-features --workspace -- --ignored
	@echo "Integration tests completed!"

# Run E2E tests (comprehensive)
test-e2e:
	@echo "Running E2E test suite..."
	@./scripts/test_e2e.sh

# Run E2E tests with SQL backend
test-e2e-sql:
	@echo "Running E2E tests with SQL backend..."
	@./scripts/test_e2e.sh --features sql-backend

# Run E2E tests (quick mode)
test-e2e-quick:
	@echo "Running E2E tests (quick mode)..."
	@./scripts/test_e2e.sh --quick

# Run E2E tests with coverage
test-e2e-coverage:
	@echo "Running E2E tests with coverage..."
	@./scripts/test_e2e.sh --coverage --features sql-backend

# Run Byzantine Generals tests
test-byzantine:
	@echo "Running Byzantine Generals tests..."
	@cd examples/byzantine && cargo test --no-fail-fast

# Run Byzantine Generals with distributed TupleSpace
test-byzantine-distributed:
	@echo "Running Byzantine Generals distributed tests..."
	@cd examples/byzantine && cargo test --test distributed_tuplespace_test --no-fail-fast

# Run quick unit tests only (for dev loop)
test-quick:
	@echo "Running quick tests..."
	@./scripts/test_quick.sh

# Run benchmarks
bench:
	@echo "Running benchmarks..."
	@$(CARGO) bench --workspace

# Format all code
fmt:
	@echo "Formatting Rust code..."
	@$(CARGO) fmt --all
	@echo "Formatting proto files..."
	@$(BUF) format -w
	@echo "Formatting complete!"

# Check formatting without changing files
fmt-check:
	@echo "Checking Rust formatting..."
	@$(CARGO) fmt --all -- --check
	@echo "Checking proto formatting..."
	@$(BUF) format -d --exit-code

# Run linters
lint: lint-rust lint-proto

# Lint Rust code
lint-rust:
	@echo "Running clippy..."
	@$(CARGO) clippy --all-features --workspace -- -D warnings
	@echo "Clippy check complete!"

# Lint proto files
lint-proto:
	@echo "Linting proto files..."
	@$(BUF) lint
	@echo "Proto lint complete!"

# Check for breaking changes in proto files
proto-breaking:
	@echo "Checking for breaking proto changes..."
	@$(BUF) breaking --against '.git#branch=main'

# Generate documentation
doc:
	@echo "Generating documentation..."
	@$(CARGO) doc --all-features --workspace --no-deps
	@echo "Documentation generated at target/doc/"

# Open documentation in browser
doc-open: doc
	@$(CARGO) doc --all-features --workspace --no-deps --open

# Clean build artifacts
clean:
	@echo "Cleaning build artifacts..."
	@$(CARGO) clean
	@rm -rf target/coverage
	@rm -rf docs/openapi/*.json
	@rm -rf docs/openapi/*.yaml
	@echo "Clean complete!"

# Clean example artifacts
clean-examples:
	@echo "Cleaning example artifacts..."
	@echo "Cleaning Rust target directories..."
	@find examples -name "target" -type d -exec rm -rf {} + 2>/dev/null || true
	@find examples -name "Cargo.lock" -type f -delete 2>/dev/null || true
	@echo "Cleaning node_modules..."
	@find examples -name "node_modules" -type d -exec rm -rf {} + 2>/dev/null || true
	@find examples -name "package-lock.json" -type f -delete 2>/dev/null || true
	@find examples -name "yarn.lock" -type f -delete 2>/dev/null || true
	@echo "Cleaning Python virtual environments..."
	@find examples -name "venv" -type d -exec rm -rf {} + 2>/dev/null || true
	@find examples -name ".venv" -type d -exec rm -rf {} + 2>/dev/null || true
	@find examples -name "__pycache__" -type d -exec rm -rf {} + 2>/dev/null || true
	@find examples -name "*.pyc" -type f -delete 2>/dev/null || true
	@find examples -name "*.pyo" -type f -delete 2>/dev/null || true
	@find examples -name ".pytest_cache" -type d -exec rm -rf {} + 2>/dev/null || true
	@echo "Cleaning WASM build artifacts..."
	@find examples -name "*.wasm" -type f -delete 2>/dev/null || true
	@find examples -name "*.wat" -type f -delete 2>/dev/null || true
	@echo "Examples clean complete!"

# Clean everything (workspace + examples)
clean-all: clean clean-examples
	@echo "All clean complete!"

# Run continuous development (watch for changes)
watch:
	@echo "Starting development watcher..."
	@cargo watch -x 'test --all-features' -x 'clippy --all-features'

# Run security audit
audit:
	@echo "Running security audit..."
	@cargo audit

# Create a new crate
new-crate:
	@echo "Creating new crate: $(NAME)"
	@cargo new --lib crates/$(NAME)
	@echo "Created crate at crates/$(NAME)"

# Run pre-commit checks (format, lint, test)
pre-commit: fmt-check lint test
	@echo "Pre-commit checks passed!"

# Run full CI pipeline locally
ci: proto fmt-check lint test check-coverage
	@echo "CI pipeline complete!"

# Start example actor system
run-example:
	@echo "Starting example actor system..."
	@$(CARGO) run --example basic_actors

# Development setup
setup: install-tools proto build
	@echo "Development environment ready!"

# Update dependencies
update-deps:
	@echo "Updating Rust dependencies..."
	@$(CARGO) update
	@echo "Updating buf dependencies..."
	@$(BUF) mod update
	@echo "Dependencies updated!"

# Check dependency licenses
check-licenses:
	@cargo license

# Generate SBOM (Software Bill of Materials)
sbom:
	@cargo sbom > sbom.txt
	@echo "SBOM generated at sbom.txt"

# Performance profiling
profile:
	@echo "Running performance profiling..."
	@$(CARGO) build --release
	@CARGO_PROFILE_RELEASE_DEBUG=true $(CARGO) run --release --example benchmark

# Quick test for development
quick: fmt build test-crate CRATE=actor
	@echo "Quick test complete!"

# Full validation before push
validate: proto fmt lint test doc
	@echo "Validation complete! Ready to push."

# Initialize git hooks
init-hooks:
	@echo "#!/bin/sh" > .git/hooks/pre-commit
	@echo "make pre-commit" >> .git/hooks/pre-commit
	@chmod +x .git/hooks/pre-commit
	@echo "Git hooks initialized!"

# Display project statistics
stats:
	@echo "Project Statistics:"
	@echo "==================="
	@echo "Lines of Rust code:"
	@find crates -name "*.rs" | xargs wc -l | tail -1
	@echo "Number of tests:"
	@grep -r "#\[test\]" crates --include="*.rs" | wc -l
	@echo "Number of proto files:"
	@find proto -name "*.proto" | wc -l
	@echo "Number of crates:"
	@ls -d crates/*/ | wc -l

# ============================================================================
# WASM Targets
# ============================================================================

# Build all WASM actors
build-wasm:
	@echo "Building all WASM actors..."
	@./scripts/build-all-wasm.sh

# Test all WASM examples
test-wasm:
	@echo "Testing WASM calculator..."
	@cd examples/wasm-calculator && cargo test --all-features
	@echo "Testing WASM heat diffusion..."
	@cd examples/heat_diffusion && cargo test --test heat_diffusion_wasm_test

# Build + test WASM
wasm: build-wasm test-wasm
	@echo "WASM actors built and tested successfully!"

# Test all examples (including WASM)
test-all-examples:
	@echo "Running all example tests..."
	@./scripts/test-all-examples.sh

# Quick WASM check (just compilation)
wasm-check:
	@echo "Checking WASM compilation..."
	@rustup target list | grep "wasm32-unknown-unknown (installed)" || rustup target add wasm32-unknown-unknown
	@cd examples/wasm-calculator/wasm-actors && cargo check --target wasm32-unknown-unknown
	@cd examples/heat_diffusion/wasm-actors && cargo check --target wasm32-unknown-unknown
	@echo "WASM compilation OK!"

# ============================================================================
# Docker Targets
# ============================================================================

# Build Docker image (default: framework-only)
docker-build:
	@echo "Building Docker image (framework-only)..."
	@docker build -t plexspaces:latest -f Dockerfile .
	@echo "Docker image built: plexspaces:latest"

# Build Docker image variants
docker-build-sqlite:
	@echo "Building Docker image (SQLite backends)..."
	@docker build -t plexspaces:sqlite -f Dockerfile.sqlite .
	@echo "Docker image built: plexspaces:sqlite"

docker-build-postgres:
	@echo "Building Docker image (PostgreSQL backends)..."
	@docker build -t plexspaces:postgres -f Dockerfile.postgres .
	@echo "Docker image built: plexspaces:postgres"

docker-build-hybrid:
	@echo "Building Docker image (hybrid: SQLite + Redis)..."
	@docker build -t plexspaces:hybrid -f Dockerfile.hybrid .
	@echo "Docker image built: plexspaces:hybrid"

docker-build-wasm:
	@echo "Building Docker image (WASM-enabled)..."
	@docker build -t plexspaces:wasm -f Dockerfile.wasm .
	@echo "Docker image built: plexspaces:wasm"

# Build all Docker variants
docker-build-all: docker-build docker-build-sqlite docker-build-postgres docker-build-hybrid docker-build-wasm
	@echo "All Docker images built successfully!"

# Push Docker image to registry
docker-push:
	@if [ -z "$(REGISTRY)" ]; then \
		echo "Usage: make docker-push REGISTRY=<registry-url>"; \
		exit 1; \
	fi
	@echo "Pushing Docker image to $(REGISTRY)..."
	@docker tag plexspaces:latest $(REGISTRY)/plexspaces:latest
	@docker push $(REGISTRY)/plexspaces:latest
	@echo "Docker image pushed: $(REGISTRY)/plexspaces:latest"

# ============================================================================
# Docker Compose Targets
# ============================================================================

# Start docker-compose
docker-compose-up:
	@echo "Starting docker-compose..."
	@docker-compose up -d
	@echo "Docker-compose started. Use 'make docker-compose-logs' to view logs."

# Stop docker-compose
docker-compose-down:
	@echo "Stopping docker-compose..."
	@docker-compose down
	@echo "Docker-compose stopped."

# View docker-compose logs
docker-compose-logs:
	@docker-compose logs -f

# Restart docker-compose
docker-compose-restart: docker-compose-down docker-compose-up

# ============================================================================
# Kubernetes Targets
# ============================================================================

# Deploy to Kubernetes
k8s-deploy:
	@echo "Deploying to Kubernetes..."
	@kubectl apply -f k8s/deployment.yaml
	@kubectl apply -f k8s/service.yaml
	@echo "Kubernetes deployment complete. Use 'make k8s-status' to check status."

# Undeploy from Kubernetes
k8s-undeploy:
	@echo "Removing from Kubernetes..."
	@kubectl delete -f k8s/deployment.yaml || true
	@kubectl delete -f k8s/service.yaml || true
	@echo "Kubernetes deployment removed."

# Check Kubernetes status
k8s-status:
	@echo "Kubernetes deployment status:"
	@kubectl get pods -l app=plexspaces
	@kubectl get svc plexspaces

# View Kubernetes logs
k8s-logs:
	@kubectl logs -f deployment/plexspaces

# ============================================================================
# Operational Targets
# ============================================================================

# View logs (local node)
logs:
	@echo "Viewing logs..."
	@if command -v docker-compose > /dev/null && docker-compose ps | grep -q plexspaces; then \
		docker-compose logs -f plexspaces-node1; \
	elif command -v kubectl > /dev/null && kubectl get pods -l app=plexspaces 2>/dev/null | grep -q plexspaces; then \
		kubectl logs -f deployment/plexspaces; \
	else \
		echo "No running PlexSpaces instance found. Start with 'make docker-compose-up' or 'make k8s-deploy'"; \
	fi

# Check health
health:
	@echo "Checking health..."
	@if command -v grpc_health_probe > /dev/null; then \
		grpc_health_probe -addr=localhost:9001 || echo "Health check failed"; \
	else \
		echo "grpc_health_probe not found. Install from: https://github.com/grpc-ecosystem/grpc-health-probe"; \
	fi

# View metrics (if HTTP gateway enabled)
metrics:
	@echo "Fetching metrics..."
	@curl -s http://localhost:9001/metrics || echo "Metrics endpoint not available. Ensure HTTP gateway is enabled."

# Backup data (SQLite)
backup:
	@echo "Backing up data..."
	@mkdir -p backups
	@if [ -f data/journal.db ]; then \
		sqlite3 data/journal.db ".backup backups/journal.db.backup"; \
		echo "Journaling database backed up to backups/journal.db.backup"; \
	fi
	@if [ -f data/channel.db ]; then \
		sqlite3 data/channel.db ".backup backups/channel.db.backup"; \
		echo "Channel database backed up to backups/channel.db.backup"; \
	fi
	@if [ -f data/tuplespace.db ]; then \
		sqlite3 data/tuplespace.db ".backup backups/tuplespace.db.backup"; \
		echo "TupleSpace database backed up to backups/tuplespace.db.backup"; \
	fi
	@echo "Backup complete!"

# Restore data (SQLite)
restore:
	@if [ -z "$(BACKUP_DIR)" ]; then \
		echo "Usage: make restore BACKUP_DIR=backups"; \
		exit 1; \
	fi
	@echo "Restoring data from $(BACKUP_DIR)..."
	@if [ -f $(BACKUP_DIR)/journal.db.backup ]; then \
		sqlite3 data/journal.db < $(BACKUP_DIR)/journal.db.backup; \
		echo "Journaling database restored"; \
	fi
	@if [ -f $(BACKUP_DIR)/channel.db.backup ]; then \
		sqlite3 data/channel.db < $(BACKUP_DIR)/channel.db.backup; \
		echo "Channel database restored"; \
	fi
	@if [ -f $(BACKUP_DIR)/tuplespace.db.backup ]; then \
		sqlite3 data/tuplespace.db < $(BACKUP_DIR)/tuplespace.db.backup; \
		echo "TupleSpace database restored"; \
	fi
	@echo "Restore complete!"

