import React from "react";
import { Fieldset } from "../forms/Fieldset";
import { FormInput } from "../forms/FormInput";
import { formatBytes } from "../../helper/formatBytes";

export interface RateLimitsTabProps {
  downloadBps: number | null | undefined;
  uploadBps: number | null | undefined;
  peerLimit: number | null | undefined;
  concurrentInitLimit: number | null | undefined;
  onDownloadBpsChange: (value: number | null) => void;
  onUploadBpsChange: (value: number | null) => void;
  onPeerLimitChange: (value: number | null) => void;
  onConcurrentInitLimitChange: (value: number | null) => void;
}

const formatLimitHelp = (
  bps: number | null | undefined,
  label: string,
): string => {
  const value = bps ?? 0;
  if (value > 0) {
    return `Limit total ${label} speed to this number of bytes per second (current ${formatBytes(value)} per second)`;
  }
  return `Limit total ${label} speed to this number of bytes per second (currently disabled)`;
};

export const RateLimitsTab: React.FC<RateLimitsTabProps> = ({
  downloadBps,
  uploadBps,
  peerLimit,
  concurrentInitLimit,
  onDownloadBpsChange,
  onUploadBpsChange,
  onPeerLimitChange,
  onConcurrentInitLimitChange,
}) => {
  return (
    <Fieldset>
      <FormInput
        label="Download rate limit"
        name="download_bps"
        inputType="number"
        value={downloadBps?.toString() ?? ""}
        onChange={(e) => {
          const val = e.target.valueAsNumber;
          onDownloadBpsChange(isNaN(val) || val <= 0 ? null : val);
        }}
        help={formatLimitHelp(downloadBps, "download")}
      />
      <FormInput
        label="Upload rate limit"
        name="upload_bps"
        inputType="number"
        value={uploadBps?.toString() ?? ""}
        onChange={(e) => {
          const val = e.target.valueAsNumber;
          onUploadBpsChange(isNaN(val) || val <= 0 ? null : val);
        }}
        help={formatLimitHelp(uploadBps, "upload")}
      />
      <FormInput
        label="Peer limit"
        name="peer_limit"
        inputType="number"
        value={peerLimit?.toString() ?? ""}
        onChange={(e) => {
          const val = e.target.valueAsNumber;
          onPeerLimitChange(isNaN(val) || val <= 0 ? null : val);
        }}
        help={`Maximum number of peers per torrent (current: ${peerLimit ?? "default"})`}
      />
      <FormInput
        label="Concurrent init limit"
        name="concurrent_init_limit"
        inputType="number"
        value={concurrentInitLimit?.toString() ?? ""}
        onChange={(e) => {
          const val = e.target.valueAsNumber;
          onConcurrentInitLimitChange(isNaN(val) || val <= 0 ? null : val);
        }}
        help={`Maximum number of torrents initializing concurrently (current: ${concurrentInitLimit ?? "default"})`}
      />
    </Fieldset>
  );
};
