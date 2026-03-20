# syntax=docker/dockerfile:1
# Multi-stage build: compile rtbit with webui from source, output minimal scratch image.

FROM rust:alpine AS builder

RUN apk update && apk add clang lld npm pkgconf musl-dev openssl-dev openssl-libs-static curl

COPY . /src/
WORKDIR /src/

ENV OPENSSL_STATIC=1

RUN --mount=type=cache,target=/usr/local/cargo/git/db \
    --mount=type=cache,target=/usr/local/cargo/registry/cache \
    --mount=type=cache,target=/usr/local/cargo/registry/index \
    cargo build --profile release-github && \
    cp target/release-github/rtbit /bin/rtbit

# ---

FROM scratch

ADD https://curl.se/ca/cacert.pem /etc/ssl/cacerts.pem
COPY --from=builder /bin/rtbit /bin/rtbit

WORKDIR /home/rtbit

ENV HOME=/home/rtbit
ENV XDG_DATA_HOME=/home/rtbit/db
ENV XDG_CACHE_HOME=/home/rtbit/cache
ENV SSL_CERT_FILE=/etc/ssl/cacerts.pem
ENV RTBIT_HTTP_API_LISTEN_ADDR=0.0.0.0:3030
ENV RTBIT_LISTEN_PORT=4240

VOLUME /home/rtbit/db
VOLUME /home/rtbit/cache
VOLUME /home/rtbit/downloads
VOLUME /home/rtbit/completed

EXPOSE 3030
EXPOSE 4240

ENTRYPOINT ["/bin/rtbit"]
CMD ["server", "start", "/home/rtbit/downloads"]
