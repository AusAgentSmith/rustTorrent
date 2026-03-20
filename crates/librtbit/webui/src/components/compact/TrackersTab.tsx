import { TorrentListItem } from "../../api-types";

interface TrackersTabProps {
  torrent: TorrentListItem | null;
}

export const TrackersTab: React.FC<TrackersTabProps> = ({ torrent }) => {
  if (!torrent) return null;

  return (
    <div className="p-3">
      <p className="text-sm text-tertiary">
        Tracker information will be available in a future update.
      </p>
      <div className="mt-3 text-sm">
        <div className="flex items-center gap-2 mb-1">
          <span className="text-secondary">Info Hash:</span>
          <code className="bg-surface-sunken px-1.5 py-0.5 rounded text-xs font-mono">
            {torrent.info_hash}
          </code>
        </div>
      </div>
    </div>
  );
};
