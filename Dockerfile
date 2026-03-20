# syntax=docker/dockerfile:1
# Multi-stage build: compile rqbit with webui from source, output minimal scratch image.

FROM rust:alpine AS builder

RUN apk update && apk add clang lld npm pkgconf musl-dev openssl-dev openssl-libs-static curl

COPY . /src/
WORKDIR /src/

ENV OPENSSL_STATIC=1

RUN --mount=type=cache,target=/usr/local/cargo/git/db \
    --mount=type=cache,target=/usr/local/cargo/registry/cache \
    --mount=type=cache,target=/usr/local/cargo/registry/index \
    cargo build --profile release-github && \
    cp target/release-github/rqbit /bin/rqbit

# ---

FROM scratch

ADD https://curl.se/ca/cacert.pem /etc/ssl/cacerts.pem
COPY --from=builder /bin/rqbit /bin/rqbit

WORKDIR /home/rqbit

ENV HOME=/home/rqbit
ENV XDG_DATA_HOME=/home/rqbit/db
ENV XDG_CACHE_HOME=/home/rqbit/cache
ENV SSL_CERT_FILE=/etc/ssl/cacerts.pem
ENV RQBIT_HTTP_API_LISTEN_ADDR=0.0.0.0:3030
ENV RQBIT_LISTEN_PORT=4240

VOLUME /home/rqbit/db
VOLUME /home/rqbit/cache
VOLUME /home/rqbit/downloads
VOLUME /home/rqbit/completed

EXPOSE 3030
EXPOSE 4240

ENTRYPOINT ["/bin/rqbit"]
CMD ["server", "start", "/home/rqbit/downloads"]
