# rqbit

A modern BitTorrent client written in Rust, targeting full **BitTorrent V2 (BEP 52)** compliance. Fast, lightweight, and built for both humans and automation.

**Website:** [rusttorrent.dev](https://rusttorrent.dev/)

## Quick Start

### Download a torrent

```bash
rqbit download 'magnet:?...'
```

### Run as a server

```bash
rqbit server start ~/Downloads
```

The Web UI is available at [http://localhost:3030/web/](http://localhost:3030/web/) and the API at [http://localhost:3030/](http://localhost:3030/).

### Docker

```bash
docker run -d --name rqbit \
  -p 3030:3030 -p 4240:4240/tcp -p 4240:4240/udp \
  -v rqbit-db:/home/rqbit/db \
  -v rqbit-cache:/home/rqbit/cache \
  -v ~/Downloads:/home/rqbit/downloads \
  ikatson/rqbit \
  server start /home/rqbit/downloads
```

Or with Docker Compose:

```yaml
services:
  rqbit:
    image: ikatson/rqbit
    ports:
      - "3030:3030"
      - "4240:4240/tcp"
      - "4240:4240/udp"
    volumes:
      - rqbit-db:/home/rqbit/db
      - rqbit-cache:/home/rqbit/cache
      - ./downloads:/home/rqbit/downloads
    environment:
      RQBIT_HTTP_API_LISTEN_ADDR: "0.0.0.0:3030"
      RQBIT_FASTRESUME: "true"

volumes:
  rqbit-db:
  rqbit-cache:
```

### Install

```bash
# Homebrew
brew install rqbit

# Cargo
cargo install rqbit

# Pre-built binaries
# https://github.com/ikatson/rqbit/releases
```

## Features

- **BitTorrent V2** — Working toward full BEP 52 compliance
- **IPv6** — Dual-stack by default, works even without IPv6 connectivity
- **HTTP API** — Full REST API with Swagger docs at `/swagger` ([see API reference](#api))
- **Arr stack compatible** — Works with Sonarr, Radarr, Prowlarr, and other *arr applications
- **Web UI** — Built-in React frontend for torrent management
- **Desktop app** — Cross-platform native app via [Tauri](https://tauri.app/)
- **Streaming** — Stream media files directly with seek support; compatible with VLC and other players via HTTP range requests
- **UPnP Media Server** — Advertise torrents to LAN devices (smart TVs, etc.)
- **DHT** — Full distributed hash table support (BEP 5) for trackerless operation
- **Fast resume** — No rehashing on restart
- **SOCKS proxy** — Route traffic through SOCKS5 proxies
- **UPnP port forwarding** — Automatic router configuration
- **Prometheus metrics** — Available at `/metrics`
- **Watch folder** — Automatically pick up `.torrent` files from a directory
- **Systemd socket activation** — On-demand startup support
- **Shell completions** — Bash, Zsh, Fish

### Performance

rqbit is designed to be lightweight and fast. The server typically runs within a few tens of megabytes of RAM, making it suitable for Raspberry Pi and other constrained environments. Users have reported saturating 20 Gbps links.

### Supported BEPs

| BEP | Description |
|-----|-------------|
| 3 | The BitTorrent Protocol Specification |
| 5 | DHT Protocol |
| 7 | IPv6 Tracker Extension |
| 9 | Extension for Peers to Send Metadata Files |
| 10 | Extension Protocol |
| 11 | Peer Exchange (PEX) |
| 12 | Multitracker Metadata Extension |
| 14 | Local Service Discovery |
| 15 | UDP Tracker Protocol |
| 20 | Peer ID Conventions |
| 23 | Tracker Returns Compact Peer Lists |
| 27 | Private Torrents |
| 29 | uTorrent Transport Protocol (uTP) |
| 32 | IPv6 Extension for DHT |
| 47 | Padding Files and Extended File Attributes |
| 52 | BitTorrent V2 *(in progress)* |
| 53 | Magnet URI Extension — Select Specific File Indices |

## API

rqbit exposes a full HTTP API at `http://localhost:3030/`. Interactive Swagger documentation is available at `/swagger` when the server is running.

Key endpoints:

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/torrents` | List all torrents |
| `POST` | `/torrents` | Add a torrent (magnet link, URL, or .torrent file) |
| `GET` | `/torrents/{id}/stats/v1` | Torrent stats |
| `GET` | `/torrents/{id}/stream/{file_idx}` | Stream a file (supports Range headers) |
| `POST` | `/torrents/{id}/pause` | Pause a torrent |
| `POST` | `/torrents/{id}/start` | Resume a torrent |
| `POST` | `/torrents/{id}/delete` | Delete torrent and files |
| `POST` | `/torrents/{id}/forget` | Remove torrent, keep files |
| `GET` | `/dht/stats` | DHT statistics |
| `GET` | `/metrics` | Prometheus metrics |

### Authentication

Set basic auth via environment variable:

```bash
RQBIT_HTTP_BASIC_AUTH_USERPASS=username:password rqbit server start ~/Downloads
```

### Adding torrents via API

```bash
# Magnet link
curl -d 'magnet:?...' http://localhost:3030/torrents

# URL to .torrent file
curl -d 'http://example.com/file.torrent' http://localhost:3030/torrents

# Local .torrent file
curl --data-binary @file.torrent http://localhost:3030/torrents
```

Query parameters: `overwrite`, `only_files_regex`, `output_folder`, `list_only`.

## Build from Source

Requires the Rust toolchain. The `webui` feature additionally requires npm.

```bash
cargo build --release
```

## License

See [LICENSE](LICENSE).
