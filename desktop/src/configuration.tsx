type PathLike = string;
type Duration = string;
type SocketAddr = string;

interface RtbitDesktopConfigDht {
  disable: boolean;
  disable_persistence: boolean;
  persistence_filename: PathLike;
}

interface RtbitDesktopConfigConnections {
  enable_tcp_listen: boolean;
  enable_tcp_outgoing: boolean;
  enable_utp: boolean;
  enable_upnp_port_forward: boolean;
  socks_proxy: string;
  listen_port: number;
  peer_connect_timeout: Duration;
  peer_read_write_timeout: Duration;
}

interface RtbitDesktopConfigPersistence {
  disable: boolean;
  folder: PathLike;
  fastresume: boolean;
}

interface RtbitDesktopConfigHttpApi {
  disable: boolean;
  listen_addr: SocketAddr;
  read_only: boolean;
  cors_enable_all: boolean;
}

interface RtbitDesktopConfigUpnp {
  disable: boolean;

  enable_server: boolean;
  server_friendly_name: string;
}

export interface LimitsConfig {
  upload_bps?: number | null;
  download_bps?: number | null;
  peer_limit?: number | null;
  concurrent_init_limit?: number | null;
}

export interface RtbitDesktopConfig {
  default_download_location: PathLike;
  disable_upload?: boolean;
  dht: RtbitDesktopConfigDht;
  connections: RtbitDesktopConfigConnections;
  upnp: RtbitDesktopConfigUpnp;
  persistence: RtbitDesktopConfigPersistence;
  http_api: RtbitDesktopConfigHttpApi;
  ratelimits: LimitsConfig;
}

export interface CurrentDesktopState {
  config: RtbitDesktopConfig | null;
  configured: boolean;
}
