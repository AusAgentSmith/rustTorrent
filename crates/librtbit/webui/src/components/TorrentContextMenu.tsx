import { useContext, useEffect, useRef, useState } from "react";
import { TorrentListItem } from "../api-types";
import { APIContext } from "../context";
import { useErrorStore } from "../stores/errorStore";
import { useTorrentStore } from "../stores/torrentStore";
import { useUIStore } from "../stores/uiStore";
import { DeleteTorrentModal } from "./modal/DeleteTorrentModal";
import {
  FaPlay,
  FaPause,
  FaCog,
  FaTrash,
  FaCopy,
  FaTag,
  FaPlus,
} from "react-icons/fa";

export interface ContextMenuState {
  x: number;
  y: number;
  torrent: TorrentListItem;
  selectedTorrents: TorrentListItem[];
}

interface TorrentContextMenuProps {
  menu: ContextMenuState;
  onClose: () => void;
}

export const TorrentContextMenu: React.FC<TorrentContextMenuProps> = ({
  menu,
  onClose,
}) => {
  const { x, y } = menu;
  const menuRef = useRef<HTMLDivElement>(null);
  const [showDelete, setShowDelete] = useState(false);
  const [showCategoryMenu, setShowCategoryMenu] = useState(false);
  const [showNewCategoryInput, setShowNewCategoryInput] = useState(false);
  const [newCategoryName, setNewCategoryName] = useState("");
  const [actionInProgress, setActionInProgress] = useState(false);
  const newCategoryInputRef = useRef<HTMLInputElement>(null);

  const API = useContext(APIContext);
  const setCloseableError = useErrorStore((s) => s.setCloseableError);
  const refreshTorrents = useTorrentStore((s) => s.refreshTorrents);
  const openDetailsModal = useUIStore((s) => s.openDetailsModal);
  const setDetailPaneTab = useUIStore((s) => s.setDetailPaneTab);
  const categories = useUIStore((s) => s.categories);
  const setCategories = useUIStore((s) => s.setCategories);

  const targets = menu.selectedTorrents;
  const isBulk = targets.length > 1;

  const hasLive = targets.some((t) => t.stats?.state === "live");
  const hasResumable = targets.some(
    (t) => t.stats?.state === "paused" || t.stats?.state === "error",
  );
  const singleTarget = targets.length === 1 ? targets[0] : null;
  const canConfigure =
    singleTarget &&
    (singleTarget.stats?.state === "paused" ||
      singleTarget.stats?.state === "live");

  // Fetch categories once when menu opens
  const categoriesFetched = useRef(false);
  useEffect(() => {
    if (categoriesFetched.current) return;
    categoriesFetched.current = true;
    API.getCategories()
      .then((cats) => setCategories(cats))
      .catch(() => {});
  }, [API, setCategories]);

  useEffect(() => {
    const handleMouseDown = (e: MouseEvent) => {
      // Ignore right-clicks — those are handled by the contextmenu event
      if (e.button === 2) return;
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        onClose();
      }
    };
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    const handleScroll = () => onClose();

    document.addEventListener("mousedown", handleMouseDown);
    document.addEventListener("keydown", handleKeyDown);
    window.addEventListener("scroll", handleScroll, true);
    return () => {
      document.removeEventListener("mousedown", handleMouseDown);
      document.removeEventListener("keydown", handleKeyDown);
      window.removeEventListener("scroll", handleScroll, true);
    };
  }, [onClose]);

  // Clamp to viewport
  const menuWidth = 220;
  const menuMaxHeight = 400;
  const left = Math.min(x, window.innerWidth - menuWidth - 8);
  const top = Math.min(y, window.innerHeight - menuMaxHeight - 8);

  const handleResume = async () => {
    setActionInProgress(true);
    for (const t of targets) {
      if (t.stats?.state === "paused" || t.stats?.state === "error") {
        try {
          await API.start(t.id);
        } catch (e: any) {
          setCloseableError({
            text: `Error starting torrent "${t.name ?? t.id}"`,
            details: e,
          });
        }
      }
    }
    refreshTorrents();
    onClose();
  };

  const handlePause = async () => {
    setActionInProgress(true);
    for (const t of targets) {
      if (t.stats?.state === "live") {
        try {
          await API.pause(t.id);
        } catch (e: any) {
          setCloseableError({
            text: `Error pausing torrent "${t.name ?? t.id}"`,
            details: e,
          });
        }
      }
    }
    refreshTorrents();
    onClose();
  };

  const handleCopyName = () => {
    const text = targets
      .map((t) => t.name ?? "")
      .filter(Boolean)
      .join("\n");
    navigator.clipboard.writeText(text);
    onClose();
  };

  const handleCopyInfoHash = () => {
    const text = targets.map((t) => t.info_hash).join("\n");
    navigator.clipboard.writeText(text);
    onClose();
  };

  const handleCopyMagnetLink = () => {
    const links = targets.map((t) => {
      let magnet = `magnet:?xt=urn:btih:${t.info_hash}`;
      if (t.name) {
        magnet += `&dn=${encodeURIComponent(t.name)}`;
      }
      return magnet;
    });
    navigator.clipboard.writeText(links.join("\n"));
    onClose();
  };

  const handleConfigure = () => {
    if (singleTarget) {
      openDetailsModal(singleTarget.id);
      setDetailPaneTab("files");
    }
    onClose();
  };

  const handleSetCategory = async (category: string | null) => {
    setActionInProgress(true);
    for (const t of targets) {
      try {
        await API.setTorrentCategory(t.id, category);
      } catch (e: any) {
        setCloseableError({
          text: `Error setting category for "${t.name ?? t.id}"`,
          details: e,
        });
      }
    }
    refreshTorrents();
    onClose();
  };

  const handleCreateAndAssignCategory = async () => {
    const name = newCategoryName.trim();
    if (!name) {
      setShowNewCategoryInput(false);
      return;
    }
    setActionInProgress(true);
    try {
      await API.createCategory(name);
      const cats = await API.getCategories();
      setCategories(cats);
    } catch {
      // category may already exist, still assign it
    }
    for (const t of targets) {
      try {
        await API.setTorrentCategory(t.id, name);
      } catch (e: any) {
        setCloseableError({
          text: `Error setting category for "${t.name ?? t.id}"`,
          details: e,
        });
      }
    }
    refreshTorrents();
    onClose();
  };

  const itemCls =
    "flex items-center gap-2 w-full px-3 py-1.5 text-sm text-left hover:bg-surface cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed";
  const iconCls = "w-3.5 h-3.5 shrink-0";
  const separator = <div className="border-t border-divider my-1" />;

  const categoryNames = Object.keys(categories).sort((a, b) =>
    a.localeCompare(b),
  );

  return (
    <>
      <div
        ref={menuRef}
        className="fixed z-50 bg-surface-raised border border-divider rounded-lg shadow-lg py-1"
        style={{ left, top, width: menuWidth, maxHeight: menuMaxHeight }}
      >
        {hasResumable && (
          <button
            className={itemCls}
            onClick={handleResume}
            disabled={actionInProgress}
          >
            <FaPlay className={`${iconCls} text-green-600`} />
            {isBulk ? "Resume Selected" : "Resume"}
          </button>
        )}
        {hasLive && (
          <button
            className={itemCls}
            onClick={handlePause}
            disabled={actionInProgress}
          >
            <FaPause className={`${iconCls} text-amber-500`} />
            {isBulk ? "Pause Selected" : "Pause"}
          </button>
        )}

        {(hasResumable || hasLive) && separator}

        {canConfigure && (
          <>
            <button className={itemCls} onClick={handleConfigure}>
              <FaCog className={`${iconCls} text-secondary`} />
              Configure Files
            </button>
            {separator}
          </>
        )}

        {/* Category submenu */}
        <div className="relative">
          <button
            className={itemCls}
            onClick={() => setShowCategoryMenu((v) => !v)}
            disabled={actionInProgress}
          >
            <FaTag className={`${iconCls} text-secondary`} />
            {isBulk ? "Set Category..." : "Set Category..."}
          </button>
          {showCategoryMenu && (
            <div className="border-t border-divider bg-surface-raised">
              <button
                className={`${itemCls} text-tertiary`}
                onClick={() => handleSetCategory(null)}
              >
                None
              </button>
              {categoryNames.map((name) => (
                <button
                  key={name}
                  className={itemCls}
                  onClick={() => handleSetCategory(name)}
                >
                  {name}
                </button>
              ))}
              {showNewCategoryInput ? (
                <div className="px-3 py-1">
                  <input
                    ref={newCategoryInputRef}
                    type="text"
                    value={newCategoryName}
                    onChange={(e) => setNewCategoryName(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") handleCreateAndAssignCategory();
                      if (e.key === "Escape") {
                        setNewCategoryName("");
                        setShowNewCategoryInput(false);
                      }
                    }}
                    onBlur={handleCreateAndAssignCategory}
                    placeholder="Category name..."
                    autoFocus
                    className="w-full px-2 py-1 text-sm bg-surface border border-divider rounded focus:outline-none focus:border-primary placeholder:text-tertiary"
                  />
                </div>
              ) : (
                <button
                  className={itemCls}
                  onClick={() => setShowNewCategoryInput(true)}
                >
                  <FaPlus className={`${iconCls} text-tertiary`} />
                  <span className="text-tertiary">New Category...</span>
                </button>
              )}
            </div>
          )}
        </div>

        {separator}

        <button className={itemCls} onClick={handleCopyName}>
          <FaCopy className={`${iconCls} text-secondary`} />
          Copy Name
        </button>
        <button className={itemCls} onClick={handleCopyInfoHash}>
          <FaCopy className={`${iconCls} text-secondary`} />
          Copy Info Hash
        </button>
        <button className={itemCls} onClick={handleCopyMagnetLink}>
          <FaCopy className={`${iconCls} text-secondary`} />
          Copy Magnet Link
        </button>

        {separator}

        <button
          className="flex items-center gap-2 w-full px-3 py-1.5 text-sm text-left text-error hover:bg-error/10 cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
          onClick={() => setShowDelete(true)}
          disabled={actionInProgress}
        >
          <FaTrash className={`${iconCls}`} />
          {isBulk ? `Delete ${targets.length} Torrents...` : "Delete..."}
        </button>
      </div>

      <DeleteTorrentModal
        show={showDelete}
        onHide={() => {
          setShowDelete(false);
          onClose();
        }}
        torrents={targets.map((t) => ({ id: t.id, name: t.name }))}
      />
    </>
  );
};
