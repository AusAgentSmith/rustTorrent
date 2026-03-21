import { JSX, useCallback, useContext, useEffect, useMemo, useState } from "react";
import { FaPause, FaPlay, FaTrash } from "react-icons/fa";
import { BsGlobe2 } from "react-icons/bs";
import { GoSearch, GoX } from "react-icons/go";
import {
  BsBodyText,
  BsBoxArrowRight,
  BsMoon,
  BsSliders2,
  BsSun,
} from "react-icons/bs";
import { HiOutlineMenu } from "react-icons/hi";
import debounce from "lodash.debounce";

// @ts-expect-error - SVG import handled by vite-plugin-svgr
import Logo from "../../assets/logo.svg?react";

import { APIContext } from "../context";
import { useUIStore } from "../stores/uiStore";
import { useTorrentStore } from "../stores/torrentStore";
import { useErrorStore } from "../stores/errorStore";
import { useIsLargeScreen } from "../hooks/useIsLargeScreen";
import { useKeyboardShortcuts } from "../hooks/useKeyboardShortcuts";
import {
  ErrorDetails,
  STATE_LIVE,
  STATE_PAUSED,
  TorrentListItem,
} from "../api-types";
import { DarkMode } from "../helper/darkMode";
import { useAuthStore } from "../stores/authStore";
import { AuthAPI } from "../http-api";
import { IndexarrAPI } from "../http-api";
import { useIndexarrStore } from "../stores/indexarrStore";
import { MagnetInput } from "./buttons/MagnetInput";
import { FileInput } from "./buttons/FileInput";
import { IconButton } from "./buttons/IconButton";
import { Button } from "./buttons/Button";
import { ConfigModal } from "./config/ConfigModal";
import { DeleteTorrentModal } from "./modal/DeleteTorrentModal";

interface ToolbarProps {
  title: string;
  version: string;
  onMultiFileSelect?: (files: File[]) => void;
  onLogsClick: () => void;
  menuButtons?: JSX.Element[];
}

const divider = "hidden lg:block w-px h-6 bg-divider mx-1";

export const Toolbar: React.FC<ToolbarProps> = ({
  title,
  version,
  onMultiFileSelect,
  onLogsClick,
  menuButtons,
}) => {
  const API = useContext(APIContext);
  const isLargeScreen = useIsLargeScreen();

  // UI store
  const selectedTorrentIds = useUIStore((state) => state.selectedTorrentIds);
  const searchQuery = useUIStore((state) => state.searchQuery);
  const setSearchQuery = useUIStore((state) => state.setSearchQuery);
  const setSidebarOpen = useUIStore((state) => state.setSidebarOpen);

  // Torrent store
  const torrents = useTorrentStore((state) => state.torrents);
  const refreshTorrents = useTorrentStore((state) => state.refreshTorrents);

  // Error store
  const setCloseableError = useErrorStore((state) => state.setCloseableError);

  // Local state
  const [disabled, setDisabled] = useState(false);
  const [showDeleteModal, setShowDeleteModal] = useState(false);
  const [torrentsToDelete, setTorrentsToDelete] = useState<
    Pick<TorrentListItem, "id" | "name">[]
  >([]);
  const [localSearch, setLocalSearch] = useState(searchQuery);
  const [isDark, setIsDark] = useState(DarkMode.isDark());
  const [configOpen, setConfigOpen] = useState(false);
  const authState = useAuthStore((s) => s.state);
  const refreshToken = useAuthStore((s) => s.refreshToken);
  const clearTokens = useAuthStore((s) => s.clearTokens);

  // Indexarr integration
  const currentPage = useUIStore((s) => s.currentPage);
  const setCurrentPage = useUIStore((s) => s.setCurrentPage);
  const indexarrEnabled = useIndexarrStore((s) => s.status?.enabled ?? false);
  const setIndexarrStatus = useIndexarrStore((s) => s.setStatus);

  useEffect(() => {
    IndexarrAPI.getStatus()
      .then(setIndexarrStatus)
      .catch(() => setIndexarrStatus({ enabled: false }));
  }, []);

  const handleLogout = async () => {
    if (refreshToken) {
      try {
        await AuthAPI.logout(refreshToken);
      } catch {
        // Ignore logout API errors — clear local state regardless
      }
    }
    clearTokens();
  };

  // Debounced search
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const debouncedSetSearch = useCallback(
    debounce((value: string) => setSearchQuery(value), 150),
    [setSearchQuery],
  );

  const handleSearchChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const value = e.target.value;
    setLocalSearch(value);
    debouncedSetSearch(value);
  };

  const clearSearch = () => {
    setLocalSearch("");
    setSearchQuery("");
  };

  const handleDarkModeToggle = () => {
    DarkMode.toggle();
    setIsDark(DarkMode.isDark());
  };

  const selectedCount = selectedTorrentIds.size;
  const hasSelection = selectedCount > 0;

  const getTorrentById = (id: number) => torrents?.find((t) => t.id === id);

  const openDeleteModal = useCallback(() => {
    const torrentsList = Array.from(selectedTorrentIds).map((id) => {
      const torrent = getTorrentById(id);
      return {
        id,
        name: torrent?.name ?? null,
      };
    });
    setTorrentsToDelete(torrentsList);
    setShowDeleteModal(true);
  }, [selectedTorrentIds, torrents]);

  // Keyboard shortcuts (global)
  const keyboardActions = useMemo(
    () => ({ onDelete: openDeleteModal }),
    [openDeleteModal],
  );
  useKeyboardShortcuts(keyboardActions);

  const runBulkAction = async (
    action: (id: number) => Promise<void>,
    skipState: string,
    errorLabel: string,
  ) => {
    setDisabled(true);
    try {
      for (const id of selectedTorrentIds) {
        const torrent = getTorrentById(id);
        if (torrent?.stats?.state === skipState) continue;
        try {
          await action(id);
          refreshTorrents();
        } catch (e) {
          setCloseableError({
            text: `Error ${errorLabel} torrent id=${id}`,
            details: e as ErrorDetails,
          });
        }
      }
    } finally {
      setDisabled(false);
    }
  };

  const pauseSelected = () =>
    runBulkAction((id) => API.pause(id), STATE_PAUSED, "pausing");
  const resumeSelected = () =>
    runBulkAction((id) => API.start(id), STATE_LIVE, "starting");

  // Hide built-in configure button when custom menuButtons are provided
  const showBuiltInConfigButton = !menuButtons || menuButtons.length === 0;

  return (
    <header className="bg-surface-raised drop-shadow-lg flex items-center gap-1 px-2 py-1.5 flex-wrap">
      {/* Mobile hamburger */}
      {!isLargeScreen && (
        <button
          onClick={() => setSidebarOpen(true)}
          className="p-1.5 text-secondary hover:text-primary cursor-pointer"
          title="Open sidebar"
        >
          <HiOutlineMenu className="w-5 h-5" />
        </button>
      )}

      {/* Logo + title */}
      <div className="flex items-center gap-1 mr-1">
        <Logo className="w-6 h-6" alt="logo" />
        <h1 className="hidden lg:flex items-center">
          <span className="text-lg font-bold">{title}</span>
          <span className="bg-primary/10 text-primary text-xs font-semibold px-1.5 py-0.5 rounded ml-1">
            v{version}
          </span>
        </h1>
      </div>

      <div className={divider} />

      {/* Add torrent buttons */}
      <MagnetInput className="grow-0 justify-center" />
      <FileInput
        className="grow-0 justify-center"
        onMultiFileSelect={onMultiFileSelect}
      />

      {/* Indexarr browse button */}
      {indexarrEnabled && (
        <button
          onClick={() =>
            setCurrentPage(currentPage === "indexarr" ? "torrents" : "indexarr")
          }
          className={`hidden lg:inline-flex items-center gap-1 px-2 py-1 text-xs font-medium rounded cursor-pointer transition-colors ${
            currentPage === "indexarr"
              ? "bg-primary text-white"
              : "text-secondary hover:text-text hover:bg-surface"
          }`}
          title="Browse Indexarr torrent index"
        >
          <BsGlobe2 className="w-3.5 h-3.5" />
          <span>Browse Index</span>
        </button>
      )}

      <div className={divider} />

      {/* Bulk action buttons */}
      <Button
        onClick={resumeSelected}
        disabled={disabled || !hasSelection}
        variant="secondary"
        size="sm"
      >
        <FaPlay className="w-2.5 h-2.5" />
        <span className="hidden lg:inline">Resume</span>
      </Button>
      <Button
        onClick={pauseSelected}
        disabled={disabled || !hasSelection}
        variant="secondary"
        size="sm"
      >
        <FaPause className="w-2.5 h-2.5" />
        <span className="hidden lg:inline">Pause</span>
      </Button>
      <Button
        onClick={openDeleteModal}
        disabled={disabled || !hasSelection}
        variant="danger"
        size="sm"
      >
        <FaTrash className="w-2.5 h-2.5" />
        <span className="hidden lg:inline">Delete</span>
      </Button>

      {hasSelection && (
        <span className="hidden lg:inline text-xs text-secondary ml-0.5">
          {selectedCount} sel
        </span>
      )}

      {/* Spacer */}
      <div className="flex-1" />

      {/* Search input */}
      <div className="relative hidden lg:block">
        <GoSearch className="absolute left-2 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-tertiary" />
        <input
          type="text"
          data-search-input
          value={localSearch}
          onChange={handleSearchChange}
          placeholder="Search..."
          className="pl-7 pr-7 py-1 w-48 text-sm bg-surface border border-divider rounded focus:outline-none focus:border-primary placeholder:text-tertiary"
        />
        {localSearch && (
          <button
            onClick={clearSearch}
            className="absolute right-1.5 top-1/2 -translate-y-1/2 p-0.5 text-tertiary hover:text-secondary rounded cursor-pointer"
          >
            <GoX className="w-3.5 h-3.5" />
          </button>
        )}
      </div>

      <div className={divider} />

      {/* Settings buttons */}
      {menuButtons?.map((b, i) => (
        <span key={i}>{b}</span>
      ))}
      {showBuiltInConfigButton && (
        <>
          <IconButton onClick={() => setConfigOpen(true)} title="Configure">
            <BsSliders2 />
          </IconButton>
          <ConfigModal
            isOpen={configOpen}
            onClose={() => setConfigOpen(false)}
            version={version}
          />
        </>
      )}
      <IconButton onClick={onLogsClick} title="View logs">
        <BsBodyText />
      </IconButton>
      <IconButton onClick={handleDarkModeToggle} title="Toggle dark mode">
        {isDark ? <BsSun /> : <BsMoon />}
      </IconButton>
      {authState === "authenticated" && (
        <IconButton onClick={handleLogout} title="Logout">
          <BsBoxArrowRight />
        </IconButton>
      )}
      {/* Delete modal */}
      <DeleteTorrentModal
        show={showDeleteModal}
        onHide={() => setShowDeleteModal(false)}
        torrents={torrentsToDelete}
      />
    </header>
  );
};
