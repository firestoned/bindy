# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT

.PHONY: help install test lint format docker-build docker-push deploy clean kind-create kind-deploy kind-test kind-cleanup docs docs-serve docs-mdbook docs-rustdoc docs-clean docs-watch docs-github-pages crds integ-test-multi-tenancy

REGISTRY ?= ghcr.io
IMAGE_NAME ?= firestoned/bindy
IMAGE_TAG ?= latest
NAMESPACE ?= dns-system
KIND_CLUSTER ?= bindy-test

help: ## Show this help message
	@echo 'Usage: make [target]'
	@echo ''
	@echo 'Available targets:'
	@awk 'BEGIN {FS = ":.*?## "} /^[a-zA-Z_-]+:.*?## / {printf "  %-20s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

install: ## Install dependencies
	@echo "Ensure Rust toolchain is installed (rustup)."
	@rustup --version || echo "Install Rust from https://rustup.rs"

crds: ## Generate CRD YAML files from Rust types
	@echo "Generating CRD YAML files from src/crd.rs..."
	@cargo run --bin crdgen
	@echo "✓ CRD YAML files generated in deploy/crds/"

test: ## Run unit tests
	cargo test --all

test-integration: ## Run integration tests (requires Kubernetes)
	cargo test --test simple_integration -- --ignored

test-integ-multi-tenancy: ## Run multi-tenancy integration tests (requires Kubernetes)
	@echo "Running multi-tenancy integration tests..."
	@./tests/run_multi_tenancy_tests.sh

test-all: test test-integration ## Run all tests (unit + integration)

test-lib: ## Run library tests only
	cargo test --lib

test-cov: ## Run tests with coverage using tarpaulin
	@command -v cargo-tarpaulin >/dev/null 2>&1 || { echo "Installing cargo-tarpaulin..."; cargo install cargo-tarpaulin; }
	cargo tarpaulin --out Html --output-dir coverage --exclude-files tests.rs --timeout 300

test-cov-view: test-cov ## Run coverage and open report
	@echo "Coverage report generated in coverage/tarpaulin-report.html"
	open coverage/tarpaulin-report.html 2>/dev/null || echo "Open coverage/tarpaulin-report.html manually"

test-cov-ci: ## Run coverage for CI (text output)
	@command -v cargo-tarpaulin >/dev/null 2>&1 || { echo "Installing cargo-tarpaulin..."; cargo install cargo-tarpaulin; }
	cargo tarpaulin --out Stdout --exclude-files tests.rs --timeout 300

lint: ## Run linting and checks
	cargo fmt -- --check
	cargo clippy -- -D warnings

format: ## Format code
	cargo fmt

docker-build: ## Build Docker image
	./scripts/build-docker-fast.sh chef

docker-build-kind: docker-build ## Build Docker image

docker-push: ## Push Docker image
	docker push $(REGISTRY)/$(IMAGE_NAME):$(IMAGE_TAG)

docker-push-kind: docker-build-kind ## Push Docker image to local kind
	kind load docker-image $(REGISTRY)/$(IMAGE_NAME):$(IMAGE_TAG) --name $(KIND_CLUSTER)

deploy-crds: ## Deploy CRDs
	kubectl create -f deploy/crds/

deploy-rbac: ## Deploy RBAC resources
	kubectl create namespace $(NAMESPACE) --dry-run=client -o yaml | kubectl apply -f -
	kubectl apply -f deploy/rbac/ -n $(NAMESPACE)

deploy-operator: ## Deploy operator
	kubectl apply -f deploy/controller/ -n $(NAMESPACE)

deploy: deploy-crds deploy-rbac deploy-operator ## Deploy everything

undeploy: ## Remove operator
	kubectl delete -f deploy/controller/ -n $(NAMESPACE) || true
	kubectl delete -f deploy/rbac/ -n $(NAMESPACE) || true
	kubectl delete -f deploy/crds/ || true

clean: ## Clean build artifacts
	cargo clean
	rm -rf target/

run-local: ## Run controller locally
	RUST_LOG=info cargo run --release

# Kind cluster targets
kind-create: ## Create Kind cluster for testing
	kind create cluster --config deploy/kind-config.yaml --name $(KIND_CLUSTER)

kind-deploy: ## Deploy to Kind cluster (creates cluster, builds, and deploys)
	./deploy/kind-deploy.sh

kind-test: ## Run tests on Kind cluster
	./deploy/kind-test.sh

kind-integration-test: ## Run full integration test suite (local build)
	./tests/integration_test.sh

kind-integration-test-ci: ## Run integration tests in CI mode (requires IMAGE_TAG env var)
	@echo "Running integration tests in CI mode..."
	@command -v kind >/dev/null 2>&1 || { echo "Error: kind not found. Install from https://kind.sigs.k8s.io/docs/user/quick-start/"; exit 1; }
	@command -v kubectl >/dev/null 2>&1 || { echo "Error: kubectl not found"; exit 1; }
	@echo "Creating Kind cluster..."
	@kind create cluster --name $(KIND_CLUSTER) --config deploy/kind-config.yaml
	@kubectl cluster-info --context kind-$(KIND_CLUSTER)
	@echo "Installing CRDs..."
	@kubectl create namespace $(NAMESPACE) || true
	@kubectl replace --force -f deploy/crds/ 2>/dev/null || kubectl create -f deploy/crds/
	@echo "Installing RBAC..."
	@kubectl apply -f deploy/rbac/
	@echo "Deploying controller with image: $(REGISTRY)/$(IMAGE_NAME):$(IMAGE_TAG)"
	@sed "s|ghcr.io/firestoned/bindy:latest|$(REGISTRY)/$(IMAGE_NAME):$(IMAGE_TAG)|g" deploy/controller/deployment.yaml | kubectl apply -f -
	@kubectl wait --for=condition=available --timeout=300s deployment/bindy -n $(NAMESPACE)
	@echo ""
	@echo "================================================"
	@echo "  Running Simple Integration Tests"
	@echo "================================================"
	@chmod +x tests/integration_test.sh
	@tests/integration_test.sh --image "$(IMAGE_TAG)" --skip-deploy || { echo "Simple integration tests failed"; kind delete cluster --name $(KIND_CLUSTER) || true; exit 1; }
	@echo ""
	@echo "================================================"
	@echo "  Running Multi-Tenancy Integration Tests"
	@echo "================================================"
	@chmod +x tests/run_multi_tenancy_tests.sh
	@tests/run_multi_tenancy_tests.sh || { echo "Multi-tenancy integration tests failed"; kind delete cluster --name $(KIND_CLUSTER) || true; exit 1; }
	@echo ""
	@echo "Cleaning up Kind cluster..."
	@kind delete cluster --name $(KIND_CLUSTER) || true
	@echo "✓ All integration tests completed successfully"

kind-cleanup: ## Delete Kind cluster
	./deploy/kind-cleanup.sh

kind-logs: ## Show controller logs from Kind cluster
	kubectl logs -n $(NAMESPACE) -l app=bindy -f --context kind-$(KIND_CLUSTER)

# Build targets
build: ## Build the Rust binary
	cargo build --release

build-debug: ## Build the Rust binary in debug mode
	cargo build

# Documentation targets
docs: export PATH := $(HOME)/.cargo/bin:$(PATH)
docs: ## Build all documentation (mdBook + rustdoc + CRD API reference)
	@echo "Building all documentation..."
	@command -v mdbook >/dev/null 2>&1 || { echo "Error: mdbook not found. Install with: cargo install mdbook"; exit 1; }
	@echo "Generating CRD API reference documentation..."
	@cargo run --bin crddoc > docs/src/reference/api.md
	@echo "Building rustdoc API documentation..."
	@cargo doc --no-deps --all-features
	@echo "Build mdBook documentation..."
	@cd docs && mdbook build
	@echo "Copying rustdoc into documentation..."
	@mkdir -p docs/target/rustdoc
	@cp -r target/doc/* docs/target/rustdoc/
	@echo "Creating rustdoc index redirect..."
	@echo '<!DOCTYPE html>' > docs/target/rustdoc/index.html
	@echo '<html>' >> docs/target/rustdoc/index.html
	@echo '<head>' >> docs/target/rustdoc/index.html
	@echo '    <meta charset="utf-8">' >> docs/target/rustdoc/index.html
	@echo '    <title>Bindy API Documentation</title>' >> docs/target/rustdoc/index.html
	@echo '    <meta http-equiv="refresh" content="0; url=bindy/index.html">' >> docs/target/rustdoc/index.html
	@echo '</head>' >> docs/target/rustdoc/index.html
	@echo '<body>' >> docs/target/rustdoc/index.html
	@echo '    <p>Redirecting to <a href="bindy/index.html">Bindy API Documentation</a>...</p>' >> docs/target/rustdoc/index.html
	@echo '</body>' >> docs/target/rustdoc/index.html
	@echo '</html>' >> docs/target/rustdoc/index.html
	@echo "Documentation built successfully in docs/target/"
	@echo "  - User guide: docs/target/index.html"
	@echo "  - API reference: docs/target/rustdoc/bindy/index.html"

docs-serve: docs ## Build and serve documentation locally
	@echo "Serving documentation at http://localhost:3000"
	@cd docs/target && python3 -m http.server 3000

docs-mdbook: ## Build mdBook documentation only
	@command -v mdbook >/dev/null 2>&1 || { echo "Installing mdbook..."; cargo install mdbook; }
	@mdbook build -d target docs
	@echo "mdBook documentation built in docs/target/"

docs-rustdoc: ## Build rustdoc API documentation only
	cargo doc --no-deps --all-features --open

docs-clean: ## Clean documentation build artifacts
	rm -rf docs/target/
	rm -rf target/doc/

docs-watch: ## Watch and rebuild mdBook documentation on changes
	@command -v mdbook >/dev/null 2>&1 || { echo "Installing mdbook..."; cargo install mdbook; }
	mdbook serve
