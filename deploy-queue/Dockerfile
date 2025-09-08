FROM docker.io/library/rust:1-bookworm as builder

WORKDIR /app

RUN cargo install cargo-auditable --locked

COPY . .

RUN cargo auditable build --release

FROM docker.io/library/debian:bookworm-slim
WORKDIR /app

RUN apt-get update && apt-get install -y \
      ca-certificates

COPY --from=builder /app/target/release/mutexbot /usr/local/bin/mutexbot
