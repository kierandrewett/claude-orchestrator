# ── Stage 1: Build the React dashboard ───────────────────────────────────────
FROM node:22-alpine AS dashboard
WORKDIR /app
COPY dashboard/package.json dashboard/yarn.lock* dashboard/package-lock.json* ./
RUN npm install
COPY dashboard/ .
RUN npm run build

# ── Stage 2: Build the Rust server ────────────────────────────────────────────
FROM rust:1.82-slim AS server
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
WORKDIR /app

# Copy workspace manifests first so cargo can fetch dependencies.
# Stub out source files so the dependency layer is cached independently.
COPY Cargo.toml Cargo.lock ./
COPY shared/Cargo.toml   ./shared/
COPY server/Cargo.toml   ./server/
COPY client/Cargo.toml   ./client/

RUN mkdir -p shared/src server/src client/src \
    && echo 'pub fn dummy() {}' > shared/src/lib.rs \
    && echo 'fn main() {}' > server/src/main.rs \
    && echo 'fn main() {}' > client/src/main.rs \
    && cargo build --release -p claude-server 2>/dev/null || true \
    && rm -rf shared/src server/src client/src

# Now build for real
COPY shared/ ./shared/
COPY server/  ./server/
COPY client/  ./client/
RUN cargo build --release -p claude-server

# ── Stage 3: Minimal runtime image ────────────────────────────────────────────
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=server /app/target/release/claude-server ./
COPY --from=dashboard /app/dist ./static

# Persist session data outside the container
VOLUME ["/app/data"]

EXPOSE 8080
ENV HOST=0.0.0.0 \
    PORT=8080 \
    DATA_DIR=/app/data

CMD ["./claude-server"]
