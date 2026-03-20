import React from "react";
import { useStatsStore } from "../../stores/statsStore";
import { formatBytes } from "../../helper/formatBytes";
import { formatSecondsToTime } from "../../helper/formatSecondsToTime";

const InfoRow = ({ label, value }: { label: string; value: string }) => (
  <div className="flex items-center">
    <span className="w-40 text-secondary text-sm">{label}</span>
    <span className="text-primary text-sm font-medium">{value}</span>
  </div>
);

export interface GeneralTabProps {
  version: string;
}

export const GeneralTab: React.FC<GeneralTabProps> = ({ version }) => {
  const stats = useStatsStore((state) => state.stats);

  return (
    <div className="space-y-3 py-2">
      <InfoRow label="Version" value={version} />
      <InfoRow
        label="Uptime"
        value={formatSecondsToTime(stats.uptime_seconds)}
      />
      <InfoRow
        label="Total Downloaded"
        value={formatBytes(stats.counters.fetched_bytes)}
      />
      <InfoRow
        label="Total Uploaded"
        value={formatBytes(stats.counters.uploaded_bytes)}
      />
      <InfoRow label="Live Peers" value={String(stats.peers.live)} />
      <InfoRow
        label="Download Speed"
        value={stats.download_speed.human_readable}
      />
      <InfoRow label="Upload Speed" value={stats.upload_speed.human_readable} />
    </div>
  );
};
