# Build stage
FROM rust:1.83 as builder

WORKDIR /workspace

# Copy only dependency manifests first to leverage Docker layer caching
COPY Cargo.toml ./
COPY Cargo.lock* ./

# Create a dummy src/main.rs to build dependencies
RUN mkdir -p src && \
    echo "fn main() {}" > src/main.rs && \
    cargo build --release && \
    rm -rf src

# Now copy the actual source code
COPY src ./src

# Build the actual controller
# Touch main.rs to ensure it's rebuilt with the real code
RUN touch src/main.rs && cargo build --release -j 2

# Runtime stage
FROM alpine:3.20

LABEL org.opencontainers.image.source="https://github.com/firestoned/bindy"
LABEL org.opencontainers.image.description="BIND9 DNS Controller for Kubernetes"
LABEL org.opencontainers.image.licenses="MIT"

# Install BIND9 and required libraries
RUN apk add --no-cache \
    bind \
    bind-tools \
    ca-certificates \
    curl \
    libgcc

# Create bind user and directories
RUN addgroup -S bind && adduser -S -G bind bind && \
    mkdir -p /etc/bind/zones && \
    mkdir -p /var/cache/bind && \
    mkdir -p /var/lib/bind && \
    chown -R bind:bind /etc/bind /var/cache/bind /var/lib/bind

# Copy the built controller from builder
COPY --from=builder /workspace/target/release/bindy /usr/local/bin/

# Set permissions
RUN chmod +x /usr/local/bin/bindy

# Run as bind user
USER bind

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8080/health || exit 1

# Start the controller
ENTRYPOINT ["/usr/local/bin/bindy"]
