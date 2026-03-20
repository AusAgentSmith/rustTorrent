import { useState } from "react";
import { RtbitWebUI } from "rtbit-webui/src/rtbit-web";
import { CurrentDesktopState, RtbitDesktopConfig } from "./configuration";
import { ConfigModal } from "./configure";
import { IconButton } from "rtbit-webui/src/components/buttons/IconButton";
import { BsSliders2 } from "react-icons/bs";
import { APIContext } from "rtbit-webui/src/context";
import { makeAPI } from "./api";

export const RtbitDesktop: React.FC<{
  version: string;
  defaultConfig: RtbitDesktopConfig;
  currentState: CurrentDesktopState;
}> = ({ version, defaultConfig, currentState }) => {
  let [configured, setConfigured] = useState<boolean>(currentState.configured);
  let [config, setConfig] = useState<RtbitDesktopConfig>(
    currentState.config ?? defaultConfig,
  );
  let [configurationOpened, setConfigurationOpened] = useState<boolean>(false);

  const configButton = (
    <IconButton
      onClick={() => {
        setConfigurationOpened(true);
      }}
    >
      <BsSliders2 />
    </IconButton>
  );

  return (
    <APIContext.Provider value={makeAPI(config)}>
      {configured && (
        <RtbitWebUI
          title={`Rtbit Desktop`}
          version={version}
          menuButtons={[configButton]}
        ></RtbitWebUI>
      )}
      <ConfigModal
        show={!configured || configurationOpened}
        handleStartReconfigure={() => {
          setConfigured(false);
        }}
        handleCancel={() => {
          setConfigurationOpened(false);
        }}
        handleConfigured={(config) => {
          setConfig(config);
          setConfigurationOpened(false);
          setConfigured(true);
        }}
        initialConfig={config}
        defaultConfig={defaultConfig}
      />
    </APIContext.Provider>
  );
};
