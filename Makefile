.PHONY: help install test lint format docker-build docker-push deploy clean kind-create kind-deploy kind-test kind-cleanup

REGISTRY ?= ghcr.io
IMAGE_NAME ?= bindy-controller
TAG ?= latest
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

test: ## Run unit tests
	cargo test --all

test-integration: ## Run integration tests (requires Kubernetes)
	cargo test --test simple_integration -- --ignored

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
	docker build -t $(REGISTRY)/$(IMAGE_NAME):$(TAG) .

docker-push: ## Push Docker image
	docker push $(REGISTRY)/$(IMAGE_NAME):$(TAG)

deploy-crds: ## Deploy CRDs
	kubectl apply -f deploy/crds/

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

kind-integration-test: ## Run full integration test suite
	./tests/integration_test.sh

kind-cleanup: ## Delete Kind cluster
	./deploy/kind-cleanup.sh

kind-logs: ## Show controller logs from Kind cluster
	kubectl logs -n $(NAMESPACE) -l app=bindy-controller -f --context kind-$(KIND_CLUSTER)

# Build targets
build: ## Build the Rust binary
	cargo build --release

build-debug: ## Build the Rust binary in debug mode
	cargo build
