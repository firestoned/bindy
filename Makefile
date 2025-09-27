.PHONY: help install test lint format docker-build docker-push deploy clean

REGISTRY ?= ghcr.io
IMAGE_NAME ?= bind9-dns-operator
TAG ?= latest
NAMESPACE ?= dns-system

help: ## Show this help message
	@echo 'Usage: make [target]'
	@echo ''
	@echo 'Available targets:'
	@awk 'BEGIN {FS = ":.*?## "} /^[a-zA-Z_-]+:.*?## / {printf "  %-20s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

install: ## Install dependencies
	poetry install

test: ## Run tests
	poetry run pytest -v

test-cov: ## Run tests with coverage
	poetry run pytest --cov=operator --cov-report=html --cov-report=term

lint: ## Run linting
	poetry run flake8 operator/
	poetry run mypy operator/

format: ## Format code
	poetry run black operator/
	poetry run isort operator/

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
	kubectl apply -f deploy/operator/ -n $(NAMESPACE)

deploy: deploy-crds deploy-rbac deploy-operator ## Deploy everything

undeploy: ## Remove operator
	kubectl delete -f deploy/operator/ -n $(NAMESPACE) || true
	kubectl delete -f deploy/rbac/ -n $(NAMESPACE) || true
	kubectl delete -f deploy/crds/ || true

clean: ## Clean build artifacts
	rm -rf dist/ build/ *.egg-info .pytest_cache .coverage htmlcov/
	find . -type d -name __pycache__ -exec rm -rf {} + 2>/dev/null || true

run-local: ## Run operator locally
	poetry run bind9-operator
