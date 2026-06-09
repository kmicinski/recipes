FROM rust:slim-bookworm AS builder

WORKDIR /build

COPY Cargo.toml Cargo.lock* ./
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates git && rm -rf /var/lib/apt/lists/*
RUN groupadd -g 1000 app && useradd -u 1000 -g app -s /bin/bash -m app

WORKDIR /app
COPY --from=builder /build/target/release/recipes /app/recipes

RUN chown -R app:app /app
USER app

EXPOSE 7001
CMD ["/app/recipes"]
