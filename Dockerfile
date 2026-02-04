FROM rust:1.93.0 AS builder
RUN apt-get update && apt-get install -y clang libclang-dev && rm -rf /var/lib/apt/lists/*
WORKDIR /usr/src/reshape
COPY . .
RUN cargo build --release

FROM debian:bullseye AS runtime
WORKDIR /usr/share/app
COPY --from=builder /usr/src/reshape/target/release/reshape /usr/local/bin/reshape
CMD ["reshape"]