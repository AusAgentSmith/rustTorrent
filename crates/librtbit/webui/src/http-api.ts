import {
  AddTorrentResponse,
  CategoryInfo,
  DhtStats,
  ErrorDetails,
  LimitsConfig,
  ListTorrentsResponse,
  PeerStatsSnapshot,
  RtbitAPI,
  SessionStats,
  TorrentDetails,
  TorrentStats,
} from "./api-types";
import { useAuthStore } from "./stores/authStore";

// --- Auth API types ---
export interface AuthStatus {
  auth_enabled: boolean;
  setup_required: boolean;
}

export interface TokenResponse {
  access_token: string;
  refresh_token: string;
  token_type: string;
  expires_in: number;
}

// Define API URL and base path
const apiUrl = (() => {
  if (window.origin === "null") {
    return "http://localhost:3030";
  }

  const url = new URL(window.location.href);

  // assume Vite devserver
  if (url.port == "3031" || url.port == "1420") {
    return `${url.protocol}//${url.hostname}:3030`;
  }

  // Remove "/web" or "/web/" from the end and also ending slash.
  const path = /(.*?)\/?(\/web\/?)?$/.exec(url.pathname)![1] ?? "";
  return path;
})();

// Get auth headers if token is available
const getAuthHeaders = (): Record<string, string> => {
  const token = useAuthStore.getState().getAccessToken();
  if (token) {
    return { Authorization: `Bearer ${token}` };
  }
  return {};
};

// Try to refresh the token, returns true if successful
const tryRefreshToken = async (): Promise<boolean> => {
  const { refreshToken, setTokens, clearTokens } = useAuthStore.getState();
  if (!refreshToken) return false;

  try {
    const url = apiUrl + "/auth/refresh";
    const response = await fetch(url, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ refresh_token: refreshToken }),
    });
    if (!response.ok) {
      clearTokens();
      return false;
    }
    const tokens: TokenResponse = await response.json();
    setTokens(tokens.access_token, tokens.refresh_token, tokens.expires_in);
    return true;
  } catch {
    clearTokens();
    return false;
  }
};

const makeBinaryRequest = async (path: string): Promise<ArrayBuffer> => {
  const url = apiUrl + path;
  const response = await fetch(url, {
    method: "GET",
    headers: {
      Accept: "application/octet-stream",
      ...getAuthHeaders(),
    },
  });

  if (response.status === 401) {
    // Try refresh
    if (await tryRefreshToken()) {
      const retry = await fetch(url, {
        method: "GET",
        headers: {
          Accept: "application/octet-stream",
          ...getAuthHeaders(),
        },
      });
      if (retry.ok) return retry.arrayBuffer();
    }
    useAuthStore.getState().clearTokens();
    throw new Error("Authentication required");
  }

  if (!response.ok) {
    throw new Error(`HTTP ${response.status}: ${response.statusText}`);
  }

  return response.arrayBuffer();
};

const makeRequest = async (
  method: string,
  path: string,
  data?: any,
  isJson?: boolean,
): Promise<any> => {
  console.log(method, path);
  const url = apiUrl + path;
  const authHeaders = getAuthHeaders();
  const options: RequestInit = {
    method,
    headers: {
      Accept: "application/json",
      ...authHeaders,
    },
  };
  if (isJson) {
    options.headers = {
      Accept: "application/json",
      "Content-Type": "application/json",
      ...authHeaders,
    };
    options.body = JSON.stringify(data);
  } else {
    options.body = data;
  }

  const error: ErrorDetails = {
    method: method,
    path: path,
    text: "",
  };

  let response: Response;

  try {
    response = await fetch(url, options);
  } catch (e) {
    error.text = "network error";
    return Promise.reject(error);
  }

  // Handle 401 — try token refresh
  if (response.status === 401) {
    if (await tryRefreshToken()) {
      // Retry with new token
      const retryHeaders = getAuthHeaders();
      const retryOptions: RequestInit = {
        ...options,
        headers: isJson
          ? {
              Accept: "application/json",
              "Content-Type": "application/json",
              ...retryHeaders,
            }
          : { Accept: "application/json", ...retryHeaders },
      };
      try {
        response = await fetch(url, retryOptions);
      } catch (e) {
        error.text = "network error";
        return Promise.reject(error);
      }
      if (response.ok) {
        return response.json();
      }
    }
    // Refresh failed or retry failed
    useAuthStore.getState().clearTokens();
    error.status = 401;
    error.statusText = "401 Unauthorized";
    error.text = "Session expired. Please log in again.";
    return Promise.reject(error);
  }

  error.status = response.status;
  error.statusText = `${response.status} ${response.statusText}`;

  if (!response.ok) {
    const errorBody = await response.text();
    try {
      const json = JSON.parse(errorBody);
      error.text =
        json.human_readable !== undefined
          ? json.human_readable
          : JSON.stringify(json, null, 2);
    } catch (e) {
      error.text = errorBody;
    }
    return Promise.reject(error);
  }
  const result = await response.json();
  return result;
};

// --- Auth API (no auth headers needed for these) ---
export const AuthAPI = {
  getStatus: async (): Promise<AuthStatus> => {
    const url = apiUrl + "/auth/status";
    const response = await fetch(url);
    if (!response.ok) {
      // If /auth/status returns 404, auth is not available (old server)
      return { auth_enabled: false, setup_required: false };
    }
    return response.json();
  },

  login: async (username: string, password: string): Promise<TokenResponse> => {
    const url = apiUrl + "/auth/login";
    const response = await fetch(url, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ username, password }),
    });
    if (!response.ok) {
      const text = await response.text();
      throw new Error(text || "Login failed");
    }
    return response.json();
  },

  setup: async (username: string, password: string): Promise<TokenResponse> => {
    const url = apiUrl + "/auth/setup";
    const response = await fetch(url, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ username, password }),
    });
    if (!response.ok) {
      const text = await response.text();
      throw new Error(text || "Setup failed");
    }
    return response.json();
  },

  logout: async (refreshToken: string): Promise<void> => {
    const url = apiUrl + "/auth/logout";
    await fetch(url, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        ...getAuthHeaders(),
      },
      body: JSON.stringify({ refresh_token: refreshToken }),
    });
  },

  changeCredentials: async (
    currentPassword: string,
    newUsername?: string,
    newPassword?: string,
  ): Promise<void> => {
    const url = apiUrl + "/auth/change_credentials";
    const response = await fetch(url, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        ...getAuthHeaders(),
      },
      body: JSON.stringify({
        current_password: currentPassword,
        new_username: newUsername || undefined,
        new_password: newPassword || undefined,
      }),
    });
    if (!response.ok) {
      const text = await response.text();
      throw new Error(text || "Failed to change credentials");
    }
  },
};

export const API: RtbitAPI & { getVersion: () => Promise<string> } = {
  getStreamLogsUrl: () => apiUrl + "/stream_logs",
  listTorrents: (opts?: {
    withStats?: boolean;
  }): Promise<ListTorrentsResponse> => {
    const url = opts?.withStats ? "/torrents?with_stats=true" : "/torrents";
    return makeRequest("GET", url);
  },
  getTorrentDetails: (index: number): Promise<TorrentDetails> => {
    return makeRequest("GET", `/torrents/${index}`);
  },
  getTorrentStats: (index: number): Promise<TorrentStats> => {
    return makeRequest("GET", `/torrents/${index}/stats/v1`);
  },
  getPeerStats: (index: number): Promise<PeerStatsSnapshot> => {
    return makeRequest("GET", `/torrents/${index}/peer_stats?state=live`);
  },
  stats: (): Promise<SessionStats> => {
    return makeRequest("GET", "/stats");
  },

  uploadTorrent: (data, opts): Promise<AddTorrentResponse> => {
    let url = "/torrents?&overwrite=true";
    if (opts?.list_only) {
      url += "&list_only=true";
    }
    if (opts?.only_files != null) {
      url += `&only_files=${opts.only_files.join(",")}`;
    }
    if (opts?.peer_opts?.connect_timeout) {
      url += `&peer_connect_timeout=${opts.peer_opts.connect_timeout}`;
    }
    if (opts?.peer_opts?.read_write_timeout) {
      url += `&peer_read_write_timeout=${opts.peer_opts.read_write_timeout}`;
    }
    if (opts?.paused) {
      url += "&paused=true";
    }
    if (opts?.initial_peers) {
      url += `&initial_peers=${opts.initial_peers.join(",")}`;
    }
    if (opts?.output_folder) {
      url += `&output_folder=${opts.output_folder}`;
    }
    if (opts?.category) {
      url += `&category=${encodeURIComponent(opts.category)}`;
    }
    if (typeof data === "string") {
      url += "&is_url=true";
    }
    return makeRequest("POST", url, data);
  },

  updateOnlyFiles: (index: number, files: number[]): Promise<void> => {
    const url = `/torrents/${index}/update_only_files`;
    return makeRequest(
      "POST",
      url,
      {
        only_files: files,
      },
      true,
    );
  },

  pause: (index: number): Promise<void> => {
    return makeRequest("POST", `/torrents/${index}/pause`);
  },

  start: (index: number): Promise<void> => {
    return makeRequest("POST", `/torrents/${index}/start`);
  },

  forget: (index: number): Promise<void> => {
    return makeRequest("POST", `/torrents/${index}/forget`);
  },

  delete: (index: number): Promise<void> => {
    return makeRequest("POST", `/torrents/${index}/delete`);
  },
  getVersion: async (): Promise<string> => {
    const r = await makeRequest("GET", "/");
    return r.version;
  },
  getTorrentStreamUrl: (
    index: number,
    file_id: number,
    filename?: string | null,
  ) => {
    let url = apiUrl + `/torrents/${index}/stream/${file_id}`;
    if (filename) {
      url += `/${filename}`;
    }
    return url;
  },
  getPlaylistUrl: (index: number) => {
    return (apiUrl || window.origin) + `/torrents/${index}/playlist`;
  },
  getTorrentHaves: async (index: number): Promise<Uint8Array> => {
    return new Uint8Array(await makeBinaryRequest(`/torrents/${index}/haves`));
  },
  getLimits: (): Promise<LimitsConfig> => {
    return makeRequest("GET", "/torrents/limits");
  },
  setLimits: (limits: LimitsConfig): Promise<void> => {
    return makeRequest("POST", "/torrents/limits", limits, true);
  },
  getDhtStats: (): Promise<DhtStats> => {
    return makeRequest("GET", "/dht/stats");
  },
  setRustLog: (value: string): Promise<void> => {
    return makeRequest("POST", "/rust_log", value);
  },
  getMetadata: async (index: number): Promise<Uint8Array> => {
    return new Uint8Array(
      await makeBinaryRequest(`/torrents/${index}/metadata`),
    );
  },
  getCategories: (): Promise<Record<string, CategoryInfo>> => {
    return makeRequest("GET", "/torrents/categories");
  },
  createCategory: (name: string, savePath?: string): Promise<void> => {
    return makeRequest(
      "POST",
      "/torrents/categories",
      { name, save_path: savePath },
      true,
    );
  },
  deleteCategory: (name: string): Promise<void> => {
    return makeRequest(
      "DELETE",
      `/torrents/categories/${encodeURIComponent(name)}`,
    );
  },
  setTorrentCategory: (
    torrentId: number,
    category: string | null,
  ): Promise<void> => {
    return makeRequest(
      "POST",
      `/torrents/${torrentId}/set_category`,
      { category },
      true,
    );
  },
};
