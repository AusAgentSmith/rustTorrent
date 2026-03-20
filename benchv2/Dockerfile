FROM rust:1-alpine AS builder
RUN apk add --no-cache musl-dev pkgconf
WORKDIR /app

# Cache dependency build
COPY Cargo.toml Cargo.lock* ./
RUN mkdir src && echo 'fn main(){}' > src/main.rs && \
    cargo build --release 2>/dev/null; rm -rf src

# Build the actual binary
COPY src/ src/
RUN cargo build --release

FROM alpine:3.21
RUN apk add --no-cache ca-certificates curl
COPY --from=builder /app/target/release/benchv2 /usr/local/bin/benchv2
ENTRYPOINT ["benchv2"]
