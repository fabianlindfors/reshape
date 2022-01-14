FROM rust:1.58.0 AS builder
WORKDIR /usr/src/reshape
COPY . .
RUN cargo build --release

FROM debian:bullseye AS runtime
WORKDIR /usr/share/app
COPY --from=builder /usr/src/reshape/target/release/reshape /usr/local/bin/reshape
CMD ["reshape"]