FROM rust:1.88-slim AS server
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
WORKDIR /app

# Copy workspace manifests so cargo can fetch deps before copying all source.
COPY Cargo.toml Cargo.lock ./
COPY crates/server/Cargo.toml           ./crates/server/
COPY crates/client/Cargo.toml           ./crates/client/
COPY crates/shared/Cargo.toml           ./crates/shared/
COPY crates/ndjson/Cargo.toml           ./crates/ndjson/
COPY crates/events/Cargo.toml           ./crates/events/
COPY crates/containers/Cargo.toml       ./crates/containers/
COPY crates/orchestrator-llm/Cargo.toml ./crates/orchestrator-llm/
COPY crates/backend-traits/Cargo.toml   ./crates/backend-traits/
COPY crates/backend-telegram/Cargo.toml ./crates/backend-telegram/
COPY crates/backend-discord/Cargo.toml  ./crates/backend-discord/
COPY crates/backend-web/Cargo.toml      ./crates/backend-web/
COPY crates/backend-stdio/Cargo.toml          ./crates/backend-stdio/
COPY crates/claude-generate-config/Cargo.toml ./crates/claude-generate-config/
COPY crates/helper/Cargo.toml                 ./crates/helper/

RUN for dir in crates/server crates/client crates/ndjson crates/events crates/containers \
        crates/orchestrator-llm crates/backend-traits crates/backend-telegram \
        crates/backend-discord crates/backend-web crates/backend-stdio \
        crates/claude-generate-config crates/helper; do \
        mkdir -p "$dir/src" && echo 'fn main() {}' > "$dir/src/main.rs"; \
    done && \
    mkdir -p crates/shared/src && echo 'pub fn dummy() {}' > crates/shared/src/lib.rs && \
    cargo build --release -p claude-server 2>/dev/null || true && \
    rm -rf crates/*/src

# Now build for real
COPY crates/ ./crates/
RUN find crates/server/src crates/shared/src crates/events/src -name '*.rs' | xargs touch \
    && cargo build --release -p claude-server

# ── Stage 3: Minimal runtime image ────────────────────────────────────────────
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=server /app/target/release/claude-server ./

VOLUME ["/app/data"]
EXPOSE 8080

CMD ["./claude-server", "run"]
