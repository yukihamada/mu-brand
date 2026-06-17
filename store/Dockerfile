FROM rust:1.88-bookworm AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release --bin mu-store

FROM debian:bookworm-slim
# openssl: signs the Apple Wallet .pkpass manifest with PASS_CERT/PASS_KEY.
# pkpass needs a PKCS#7 detached CMS signature over manifest.json; pure-Rust
# CMS crates are immature so we shell out to /usr/bin/openssl.
# gosu: drop from root to the unprivileged `app` user at runtime (see entrypoint).
RUN apt-get update && apt-get install -y ca-certificates openssl gosu && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/mu-store .
COPY static static
# Unprivileged runtime user. We can't simply `USER app` because /data is a
# volume mounted root-owned at runtime; the entrypoint (running as root) chowns
# /data, then execs the server as `app` via gosu — so the process is non-root.
RUN useradd -r -u 10001 -m -d /home/app app
COPY docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
RUN chmod +x /usr/local/bin/docker-entrypoint.sh
VOLUME ["/data"]
ENV DB_PATH=/data/products.db PORT=3000
EXPOSE 3000
ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
CMD ["./mu-store"]
