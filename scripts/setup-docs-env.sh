#!/bin/bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT

# Setup script for Bindy documentation development environment
# This script uses Poetry to manage Python dependencies for building
# and serving the documentation.
#
# All documentation-related files are in the docs/ directory to keep
# the project root clean.

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Print with color
print_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Check if running from project root
if [ ! -f "docs/mkdocs.yml" ] || [ ! -f "docs/pyproject.toml" ]; then
    print_error "docs/mkdocs.yml or docs/pyproject.toml not found."
    print_error "Please run this script from the project root directory."
    exit 1
fi

print_info "Setting up documentation development environment with Poetry..."
print_info "All documentation files are in the docs/ directory"

# Check if Poetry is installed
if ! command -v poetry &> /dev/null; then
    print_error "Poetry is not installed."
    echo ""
    echo "Please install Poetry first:"
    echo "  curl -sSL https://install.python-poetry.org | python3 -"
    echo ""
    echo "Or visit: https://python-poetry.org/docs/#installation"
    exit 1
fi

POETRY_VERSION=$(poetry --version 2>&1 | awk '{print $3}' | tr -d '()')
print_info "Detected Poetry version: $POETRY_VERSION"

# Check Python version
PYTHON_VERSION=$(python3 --version 2>&1 | awk '{print $2}')
print_info "Detected Python version: $PYTHON_VERSION"

# Change to docs directory
cd docs

# Configure Poetry to create virtualenv in project directory
print_info "Configuring Poetry to use in-project virtualenv..."
poetry config virtualenvs.in-project true

# Check if virtualenv already exists
if [ -d ".venv" ]; then
    print_warning "Virtual environment already exists at docs/.venv"
    read -p "Do you want to recreate it? (y/N) " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        print_info "Removing existing virtual environment..."
        rm -rf .venv
        print_info "Installing dependencies with Poetry..."
        poetry install
    else
        print_info "Using existing virtual environment"
        print_info "Updating dependencies with Poetry..."
        poetry install --sync
    fi
else
    print_info "Installing dependencies with Poetry..."
    poetry install
fi

# Verify installation
print_info "Verifying installation..."
if poetry run mkdocs --version &> /dev/null; then
    MKDOCS_VERSION=$(poetry run mkdocs --version 2>&1 | awk '{print $3}' | head -1)
    print_info "MkDocs $MKDOCS_VERSION installed successfully"
else
    print_error "MkDocs installation failed"
    exit 1
fi

# Return to project root
cd ..

# Print success message
echo ""
print_info "✅ Documentation environment setup complete!"
echo ""
echo "All documentation files are in the docs/ directory:"
echo "  docs/pyproject.toml     - Poetry dependencies"
echo "  docs/mkdocs.yml         - MkDocs configuration"
echo "  docs/.venv              - Python virtual environment"
echo "  docs/src/               - Markdown content"
echo ""
echo "To activate the Poetry environment:"
echo "  cd docs && poetry shell"
echo ""
echo "To run commands without activating:"
echo "  cd docs && poetry run mkdocs serve"
echo "  cd docs && poetry run mkdocs build"
echo ""
echo "To build documentation from project root:"
echo "  make docs"
echo ""
echo "To serve documentation locally with live reload:"
echo "  make docs-serve"
echo ""
echo "To deactivate the Poetry shell:"
echo "  exit"
echo ""
