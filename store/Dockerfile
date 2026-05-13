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
RUN apt-get update && apt-get install -y ca-certificates openssl && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/mu-store .
COPY static static
VOLUME ["/data"]
ENV DB_PATH=/data/products.db PORT=3000
EXPOSE 3000
CMD ["./mu-store"]
