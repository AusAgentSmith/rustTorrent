import { StrictMode } from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { CurrentDesktopState, RtbitDesktopConfig } from "./configuration";
import { RtbitDesktop } from "./rtbit-desktop";
import { APIContext } from "rtbit-webui/src/context";
import { RtbitWebUI } from "rtbit-webui/src/rtbit-web";
import * as HttpApi from "rtbit-webui/src/http-api";

import "./styles/index.css";

async function get_version(): Promise<string> {
  return invoke<string>("get_version");
}

async function get_default_config(): Promise<RtbitDesktopConfig> {
  return invoke<RtbitDesktopConfig>("config_default");
}

async function get_current_config(): Promise<CurrentDesktopState> {
  return invoke<CurrentDesktopState>("config_current");
}

Promise.all([get_version(), get_default_config(), get_current_config()])
  .then(([version, defaultConfig, currentState]) => {
    console.log(version, defaultConfig, currentState);
    ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
      <StrictMode>
        <RtbitDesktop
          version={version}
          defaultConfig={defaultConfig}
          currentState={currentState}
        />
      </StrictMode>,
    );
  })
  .catch((e) => {
    console.log(e);

    // Fallback to HTTP API at localhost:3030
    ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
      <StrictMode>
        <APIContext.Provider value={HttpApi.API}>
          <RtbitWebUI title="rtbit" version="desktop fallback" />
        </APIContext.Provider>
      </StrictMode>,
    );
  });
