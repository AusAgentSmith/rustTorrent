import { useContext, useEffect, useMemo } from "react";
import { BsTag, BsTagFill } from "react-icons/bs";
import { APIContext } from "../../context";
import { useUIStore } from "../../stores/uiStore";
import { useTorrentStore } from "../../stores/torrentStore";

export const CategoryFilter: React.FC = () => {
  const API = useContext(APIContext);
  const torrents = useTorrentStore((state) => state.torrents);
  const categoryFilter = useUIStore((state) => state.categoryFilter);
  const setCategoryFilter = useUIStore((state) => state.setCategoryFilter);
  const categories = useUIStore((state) => state.categories);
  const setCategories = useUIStore((state) => state.setCategories);

  // Fetch categories on mount
  useEffect(() => {
    let cancelled = false;
    API.getCategories()
      .then((cats) => {
        if (!cancelled) setCategories(cats);
      })
      .catch(() => {
        // Categories endpoint may not exist yet on the backend
      });
    return () => {
      cancelled = true;
    };
  }, [API, setCategories]);

  // Count torrents per category
  const categoryCounts = useMemo(() => {
    const counts: Record<string, number> = {};
    let uncategorized = 0;
    let total = 0;

    if (torrents) {
      for (const t of torrents) {
        total++;
        if (t.category) {
          counts[t.category] = (counts[t.category] || 0) + 1;
        } else {
          uncategorized++;
        }
      }
    }

    return { counts, uncategorized, total };
  }, [torrents]);

  // Build list of category names (from both API categories and torrent data)
  const categoryNames = useMemo(() => {
    const names = new Set<string>();
    for (const name of Object.keys(categories)) {
      names.add(name);
    }
    for (const name of Object.keys(categoryCounts.counts)) {
      names.add(name);
    }
    return Array.from(names).sort((a, b) => a.localeCompare(b));
  }, [categories, categoryCounts.counts]);

  if (categoryNames.length === 0 && categoryCounts.uncategorized === 0) {
    return null;
  }

  const activeItemClass = "bg-primary/10 text-primary font-medium";
  const inactiveItemClass =
    "text-secondary hover:bg-surface-sunken hover:text-primary";
  const iconClass = "w-3.5 h-3.5 shrink-0";

  return (
    <div>
      <div className="px-3 pt-3 pb-1">
        <h3 className="text-xs font-semibold text-tertiary uppercase tracking-wider">
          Categories
        </h3>
      </div>
      <div className="px-1.5">
        {/* All */}
        <button
          onClick={() => setCategoryFilter(null)}
          className={`w-full flex items-center gap-2.5 px-2.5 py-1.5 rounded text-sm cursor-pointer transition-colors ${
            categoryFilter === null ? activeItemClass : inactiveItemClass
          }`}
        >
          <BsTag className={iconClass} />
          <span className="flex-1 text-left">All</span>
          <span
            className={`text-xs tabular-nums ${
              categoryFilter === null ? "text-primary" : "text-tertiary"
            }`}
          >
            {categoryCounts.total}
          </span>
        </button>

        {/* Uncategorized */}
        <button
          onClick={() => setCategoryFilter("")}
          className={`w-full flex items-center gap-2.5 px-2.5 py-1.5 rounded text-sm cursor-pointer transition-colors ${
            categoryFilter === "" ? activeItemClass : inactiveItemClass
          }`}
        >
          <BsTag className={iconClass} />
          <span className="flex-1 text-left">Uncategorized</span>
          <span
            className={`text-xs tabular-nums ${
              categoryFilter === "" ? "text-primary" : "text-tertiary"
            }`}
          >
            {categoryCounts.uncategorized}
          </span>
        </button>

        {/* Each category */}
        {categoryNames.map((name) => (
          <button
            key={name}
            onClick={() => setCategoryFilter(name)}
            className={`w-full flex items-center gap-2.5 px-2.5 py-1.5 rounded text-sm cursor-pointer transition-colors ${
              categoryFilter === name ? activeItemClass : inactiveItemClass
            }`}
          >
            <BsTagFill className={iconClass} />
            <span className="flex-1 text-left truncate">{name}</span>
            <span
              className={`text-xs tabular-nums ${
                categoryFilter === name ? "text-primary" : "text-tertiary"
              }`}
            >
              {categoryCounts.counts[name] ?? 0}
            </span>
          </button>
        ))}
      </div>
    </div>
  );
};
