import { TorrentListItem, STATE_INITIALIZING } from "../../api-types";
import { StatusIcon } from "../StatusIcon";
import { formatBytes } from "../../helper/formatBytes";
import { getCompletionETA } from "../../helper/getCompletionETA";
import { memo } from "react";
import { ColumnDef, ColumnId, useColumnStore } from "../../stores/columnStore";

interface TorrentTableRowProps {
  torrent: TorrentListItem;
  isSelected: boolean;
  onRowClick: (id: number, e: React.MouseEvent) => void;
  onContextMenu: (id: number, e: React.MouseEvent) => void;
  onCheckboxChange: (id: number) => void;
  visibleColumns: ColumnDef[];
}

/** Shared colgroup matching the header */
function RowColGroup({ columns }: { columns: ColumnDef[] }) {
  const getWidth = useColumnStore((s) => s.getWidth);
  return (
    <colgroup>
      {columns.map((col) => {
        const w = getWidth(col.id);
        return (
          <col key={col.id} style={w > 0 ? { width: `${w}px` } : undefined} />
        );
      })}
    </colgroup>
  );
}

const TorrentTableRowUnmemoized: React.FC<TorrentTableRowProps> = ({
  torrent,
  isSelected,
  onRowClick,
  onContextMenu,
  onCheckboxChange,
  visibleColumns,
}) => {
  const stats = torrent.stats;
  const state = stats?.state ?? "";
  const error = stats?.error ?? null;
  const totalBytes = stats?.total_bytes ?? 1;
  const progressBytes = stats?.progress_bytes ?? 0;
  const finished = stats?.finished || false;
  const live = !!stats?.live;

  const progressPercentage = error
    ? 100
    : totalBytes === 0
      ? 100
      : Math.round((progressBytes / totalBytes) * 100);

  const downloadSpeed = stats?.live?.download_speed?.human_readable ?? "-";
  const uploadSpeed = stats?.live?.upload_speed?.human_readable ?? "-";
  const uploadedBytes = stats?.live?.snapshot.uploaded_bytes ?? 0;

  const peerStats = stats?.live?.snapshot.peer_stats;
  const peersDisplay = peerStats ? `${peerStats.live}/${peerStats.seen}` : "-";

  const eta = stats ? getCompletionETA(stats) : "-";
  const displayEta = finished ? "Done" : eta;

  const name = torrent.name ?? "";

  const handleRowClick = (e: React.MouseEvent) => {
    onRowClick(torrent.id, e);
  };

  const handleContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    onContextMenu(torrent.id, e);
  };

  const handleCheckboxClick = (e: React.MouseEvent) => {
    e.stopPropagation();
    onCheckboxChange(torrent.id);
  };

  const cellBorder = "border-r border-divider/40";

  function renderCell(col: ColumnDef): React.ReactNode {
    const alignClass =
      col.align === "center"
        ? "text-center"
        : col.align === "right"
          ? "text-right"
          : "text-left";
    const baseCls = `px-2 align-middle whitespace-nowrap ${cellBorder}`;

    switch (col.id as ColumnId) {
      case "checkbox":
        return (
          <td
            key="checkbox"
            className={`px-2 align-middle text-center ${cellBorder}`}
            onMouseDown={handleCheckboxClick}
          >
            <input
              type="checkbox"
              checked={isSelected}
              onChange={() => {}}
              className="w-4 h-4 rounded border-divider-strong bg-surface text-primary focus:ring-primary"
            />
          </td>
        );
      case "status_icon":
        return (
          <td key="status_icon" className={`px-1 align-middle ${cellBorder}`}>
            <StatusIcon
              className="w-5 h-5"
              error={!!error}
              live={live}
              finished={finished}
            />
          </td>
        );
      case "id":
        return (
          <td
            key="id"
            className={`${baseCls} text-center text-tertiary font-mono`}
          >
            {torrent.id}
          </td>
        );
      case "name":
        return (
          <td key="name" className={`px-2 align-middle ${cellBorder}`}>
            <div className="truncate" title={name}>
              {name || "Loading..."}
            </div>
            {error && (
              <div className="truncate text-sm text-error" title={error}>
                {error}
              </div>
            )}
          </td>
        );
      case "size":
        return (
          <td key="size" className={`${baseCls} ${alignClass} text-secondary`}>
            {formatBytes(totalBytes)}
          </td>
        );
      case "progress":
        return (
          <td
            key="progress"
            className={`px-2 align-middle text-center ${cellBorder}`}
          >
            <div className="flex items-center gap-2">
              <div className="flex-1 h-1.5 bg-divider rounded-full overflow-hidden">
                <div
                  className={`h-full rounded-full ${
                    error
                      ? "bg-error-bg"
                      : finished
                        ? "bg-success-bg"
                        : state === STATE_INITIALIZING
                          ? "bg-warning-bg"
                          : "bg-primary-bg"
                  }`}
                  style={{ width: `${progressPercentage}%` }}
                />
              </div>
              <span className="text-sm text-secondary w-8 text-right">
                {progressPercentage}%
              </span>
            </div>
          </td>
        );
      case "downloadedBytes":
        return (
          <td
            key="downloadedBytes"
            className={`${baseCls} ${alignClass} text-secondary`}
          >
            {formatBytes(progressBytes)}
          </td>
        );
      case "downSpeed":
        return (
          <td
            key="downSpeed"
            className={`${baseCls} ${alignClass} text-secondary`}
          >
            {downloadSpeed}
          </td>
        );
      case "upSpeed":
        return (
          <td
            key="upSpeed"
            className={`${baseCls} ${alignClass} text-secondary`}
          >
            {uploadSpeed}
          </td>
        );
      case "uploadedBytes":
        return (
          <td
            key="uploadedBytes"
            className={`${baseCls} ${alignClass} text-secondary`}
          >
            {uploadedBytes > 0 ? formatBytes(uploadedBytes) : ""}
          </td>
        );
      case "eta":
        return (
          <td key="eta" className={`${baseCls} ${alignClass} text-secondary`}>
            {displayEta}
          </td>
        );
      case "peers":
        return (
          <td key="peers" className={`${baseCls} ${alignClass} text-secondary`}>
            {peersDisplay}
          </td>
        );
      case "state":
        return (
          <td
            key="state"
            className={`${baseCls} ${alignClass} text-secondary capitalize`}
          >
            {state}
          </td>
        );
      case "info_hash":
        return (
          <td
            key="info_hash"
            className={`px-2 align-middle font-mono text-xs text-tertiary ${cellBorder}`}
          >
            <div className="truncate" title={torrent.info_hash}>
              {torrent.info_hash}
            </div>
          </td>
        );
      case "ratio": {
        const ratio =
          totalBytes > 0 ? (uploadedBytes / totalBytes).toFixed(2) : "0.00";
        return (
          <td key="ratio" className={`${baseCls} ${alignClass} text-secondary`}>
            {ratio}
          </td>
        );
      }
      case "category":
        return (
          <td
            key="category"
            className={`${baseCls} ${alignClass} text-secondary`}
          >
            <span className="truncate">{torrent.category || "\u2014"}</span>
          </td>
        );
      default:
        return <td key={col.id} className={baseCls} />;
    }
  }

  return (
    <table className="w-full table-fixed">
      <RowColGroup columns={visibleColumns} />
      <tbody>
        <tr
          onMouseDown={handleRowClick}
          onContextMenu={handleContextMenu}
          className={`cursor-pointer border-b border-divider text-sm h-8 ${
            isSelected ? "bg-primary/10" : "hover:bg-surface-raised"
          }`}
        >
          {visibleColumns.map((col) => renderCell(col))}
        </tr>
      </tbody>
    </table>
  );
};

export const TorrentTableRow = memo(TorrentTableRowUnmemoized);
