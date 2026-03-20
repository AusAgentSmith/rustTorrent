import { create } from "zustand";

export type ColumnId =
  | "checkbox"
  | "status_icon"
  | "id"
  | "name"
  | "size"
  | "progress"
  | "downloadedBytes"
  | "downSpeed"
  | "upSpeed"
  | "uploadedBytes"
  | "eta"
  | "peers"
  | "state"
  | "info_hash"
  | "ratio";

export interface ColumnDef {
  id: ColumnId;
  label: string;
  defaultWidth: number; // 0 means flex (takes remaining space)
  minWidth: number;
  align: "left" | "center" | "right";
  defaultVisible: boolean;
  configurable: boolean; // false for checkbox, status_icon
  sortable: boolean;
}

export const COLUMN_DEFS: ColumnDef[] = [
  {
    id: "checkbox",
    label: "",
    defaultWidth: 32,
    minWidth: 32,
    align: "center",
    defaultVisible: true,
    configurable: false,
    sortable: false,
  },
  {
    id: "status_icon",
    label: "",
    defaultWidth: 32,
    minWidth: 32,
    align: "center",
    defaultVisible: true,
    configurable: false,
    sortable: false,
  },
  {
    id: "id",
    label: "ID",
    defaultWidth: 48,
    minWidth: 36,
    align: "center",
    defaultVisible: true,
    configurable: true,
    sortable: true,
  },
  {
    id: "name",
    label: "Name",
    defaultWidth: 0,
    minWidth: 100,
    align: "left",
    defaultVisible: true,
    configurable: true,
    sortable: true,
  },
  {
    id: "size",
    label: "Size",
    defaultWidth: 80,
    minWidth: 60,
    align: "right",
    defaultVisible: true,
    configurable: true,
    sortable: true,
  },
  {
    id: "progress",
    label: "Progress",
    defaultWidth: 120,
    minWidth: 80,
    align: "center",
    defaultVisible: true,
    configurable: true,
    sortable: true,
  },
  {
    id: "downloadedBytes",
    label: "Recv",
    defaultWidth: 80,
    minWidth: 60,
    align: "right",
    defaultVisible: true,
    configurable: true,
    sortable: true,
  },
  {
    id: "downSpeed",
    label: "↓ Speed",
    defaultWidth: 80,
    minWidth: 60,
    align: "right",
    defaultVisible: true,
    configurable: true,
    sortable: true,
  },
  {
    id: "upSpeed",
    label: "↑ Speed",
    defaultWidth: 80,
    minWidth: 60,
    align: "right",
    defaultVisible: true,
    configurable: true,
    sortable: true,
  },
  {
    id: "uploadedBytes",
    label: "Sent",
    defaultWidth: 80,
    minWidth: 60,
    align: "right",
    defaultVisible: true,
    configurable: true,
    sortable: true,
  },
  {
    id: "eta",
    label: "ETA",
    defaultWidth: 80,
    minWidth: 60,
    align: "center",
    defaultVisible: true,
    configurable: true,
    sortable: true,
  },
  {
    id: "peers",
    label: "Peers",
    defaultWidth: 64,
    minWidth: 50,
    align: "center",
    defaultVisible: true,
    configurable: true,
    sortable: true,
  },
  {
    id: "state",
    label: "State",
    defaultWidth: 80,
    minWidth: 60,
    align: "center",
    defaultVisible: false,
    configurable: true,
    sortable: true,
  },
  {
    id: "info_hash",
    label: "Info Hash",
    defaultWidth: 150,
    minWidth: 80,
    align: "left",
    defaultVisible: false,
    configurable: true,
    sortable: false,
  },
  {
    id: "ratio",
    label: "Ratio",
    defaultWidth: 64,
    minWidth: 50,
    align: "right",
    defaultVisible: false,
    configurable: true,
    sortable: true,
  },
];

const STORAGE_KEY_WIDTHS = "rqbit-column-widths";
const STORAGE_KEY_VISIBLE = "rqbit-column-visible";

function loadJson(key: string): Record<string, any> {
  try {
    const stored = localStorage.getItem(key);
    return stored ? JSON.parse(stored) : {};
  } catch {
    return {};
  }
}

export interface ColumnStore {
  columnWidths: Record<string, number>;
  columnVisibility: Record<string, boolean>;

  getWidth: (id: ColumnId) => number;
  isVisible: (id: ColumnId) => boolean;
  getVisibleColumns: () => ColumnDef[];

  setColumnWidth: (id: ColumnId, width: number) => void;
  toggleColumnVisibility: (id: ColumnId) => void;
  resetColumns: () => void;
}

export const useColumnStore = create<ColumnStore>((set, get) => ({
  columnWidths: loadJson(STORAGE_KEY_WIDTHS),
  columnVisibility: loadJson(STORAGE_KEY_VISIBLE),

  getWidth: (id) => {
    const w = get().columnWidths[id];
    if (w !== undefined) return w;
    return COLUMN_DEFS.find((c) => c.id === id)?.defaultWidth ?? 80;
  },

  isVisible: (id) => {
    const v = get().columnVisibility[id];
    if (v !== undefined) return v;
    return COLUMN_DEFS.find((c) => c.id === id)?.defaultVisible ?? true;
  },

  getVisibleColumns: () => {
    return COLUMN_DEFS.filter((c) => get().isVisible(c.id));
  },

  setColumnWidth: (id, width) => {
    const def = COLUMN_DEFS.find((c) => c.id === id);
    const minWidth = def?.minWidth ?? 30;
    const clampedWidth = Math.max(minWidth, width);

    set((state) => {
      const newWidths = { ...state.columnWidths, [id]: clampedWidth };
      localStorage.setItem(STORAGE_KEY_WIDTHS, JSON.stringify(newWidths));
      return { columnWidths: newWidths };
    });
  },

  toggleColumnVisibility: (id) => {
    const def = COLUMN_DEFS.find((c) => c.id === id);
    if (!def?.configurable) return;

    set((state) => {
      const current = state.columnVisibility[id] ?? def.defaultVisible;
      const newVisible = { ...state.columnVisibility, [id]: !current };
      localStorage.setItem(STORAGE_KEY_VISIBLE, JSON.stringify(newVisible));
      return { columnVisibility: newVisible };
    });
  },

  resetColumns: () => {
    localStorage.removeItem(STORAGE_KEY_WIDTHS);
    localStorage.removeItem(STORAGE_KEY_VISIBLE);
    set({ columnWidths: {}, columnVisibility: {} });
  },
}));
