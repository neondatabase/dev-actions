FROM docker.io/library/rust:1-bookworm as builder

WORKDIR /app

COPY . .

RUN cargo build --release

FROM docker.io/library/debian:bookworm-slim
WORKDIR /app

RUN apt-get update && apt-get install -y \
      ca-certificates

COPY --from=builder /app/target/release/mutexbot /usr/local/bin/mutexbot
