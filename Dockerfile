FROM python:3.11-slim

LABEL org.opencontainers.image.source="https://github.com/yourusername/bind9-dns-operator"
LABEL org.opencontainers.image.description="BIND9 DNS Operator for Kubernetes"
LABEL org.opencontainers.image.licenses="MIT"

# Install system dependencies
RUN apt-get update && apt-get install -y \
    dnsutils \
    curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Install Poetry
RUN pip install poetry==1.7.1

# Copy dependency files
COPY pyproject.toml ./

# Install dependencies without dev packages
RUN poetry config virtualenvs.create false \
    && poetry install --no-dev --no-interaction --no-ansi

# Copy operator code
COPY operator/ ./operator/

# Create non-root user
RUN groupadd -r operator && useradd -r -g operator operator \
    && chown -R operator:operator /app

USER operator

# Health check
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -f http://localhost:8080/healthz || exit 1

ENTRYPOINT ["python", "-m", "operator.main"]
