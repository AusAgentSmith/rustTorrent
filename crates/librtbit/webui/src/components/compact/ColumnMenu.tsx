import { useEffect, useRef } from "react";
import {
  COLUMN_DEFS,
  useColumnStore,
  ColumnId,
} from "../../stores/columnStore";

interface ColumnMenuProps {
  x: number;
  y: number;
  onClose: () => void;
}

export const ColumnMenu: React.FC<ColumnMenuProps> = ({ x, y, onClose }) => {
  const isVisible = useColumnStore((s) => s.isVisible);
  const toggleColumnVisibility = useColumnStore(
    (s) => s.toggleColumnVisibility,
  );
  const resetColumns = useColumnStore((s) => s.resetColumns);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const handleClick = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        onClose();
      }
    };
    const handleEsc = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("mousedown", handleClick);
    document.addEventListener("keydown", handleEsc);
    return () => {
      document.removeEventListener("mousedown", handleClick);
      document.removeEventListener("keydown", handleEsc);
    };
  }, [onClose]);

  // Clamp position to viewport
  const menuWidth = 200;
  const menuMaxHeight = 400;
  const left = Math.min(x, window.innerWidth - menuWidth - 8);
  const top = Math.min(y, window.innerHeight - menuMaxHeight - 8);

  const configurableColumns = COLUMN_DEFS.filter((c) => c.configurable);

  return (
    <div
      ref={menuRef}
      className="fixed z-50 bg-surface-raised border border-divider rounded-lg shadow-lg py-1 overflow-y-auto"
      style={{ left, top, width: menuWidth, maxHeight: menuMaxHeight }}
    >
      <div className="px-3 py-1.5 text-xs font-semibold text-tertiary uppercase tracking-wider border-b border-divider">
        Columns
      </div>
      {configurableColumns.map((col) => {
        const visible = isVisible(col.id);
        return (
          <label
            key={col.id}
            className="flex items-center gap-2 px-3 py-1.5 hover:bg-surface cursor-pointer text-sm"
          >
            <input
              type="checkbox"
              checked={visible}
              onChange={() => toggleColumnVisibility(col.id as ColumnId)}
              className="w-3.5 h-3.5 rounded border-divider-strong bg-surface text-primary focus:ring-primary"
            />
            <span className={visible ? "text-primary" : "text-tertiary"}>
              {col.label}
            </span>
          </label>
        );
      })}
      <div className="border-t border-divider mt-1 pt-1">
        <button
          onClick={() => {
            resetColumns();
            onClose();
          }}
          className="w-full text-left px-3 py-1.5 text-xs text-secondary hover:bg-surface cursor-pointer"
        >
          Reset to defaults
        </button>
      </div>
    </div>
  );
};
