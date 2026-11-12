import { Globe, Laptop, Search, WifiOff } from "lucide-react";
import { useEffect, useState } from "react";
import { api, type WebWorkflowSummary } from "../api";
import { t } from "../i18n";
import { useWeb } from "../stores/web";
import { useWorkflows } from "../stores/workflows";

export type SearchPick = { kind: "local" | "web"; id: string };

/** ⌘K 찾기 — 내 PC·웹 이음 워크플로우 이름(대소문자 무시, 부분 일치). 웹을 못 받으면 내 PC만. */
export function SearchDialog({ open, onClose, onPick }: { open: boolean; onClose: () => void; onPick: (item: SearchPick) => void }) {
  const local = useWorkflows((s) => s.list);
  const loadLocal = useWorkflows((s) => s.load);
  const guard = useWeb((s) => s.guard);
  const [web, setWeb] = useState<WebWorkflowSummary[] | null>([]);
  const [q, setQ] = useState("");
  const [sel, setSel] = useState(0);

  useEffect(() => {
    if (!open) return;
    setQ("");
    setSel(0);
    void loadLocal();
    guard(api.webListWorkflows()).then(setWeb, () => setWeb(null));
  }, [open, loadLocal, guard]);

  if (!open) return null;

  const needle = q.trim().toLowerCase();
  const hit = (name: string) => name.toLowerCase().includes(needle);
  const items: (SearchPick & { name: string })[] = [
    ...local.filter((w) => hit(w.name)).map((w) => ({ kind: "local" as const, id: w.id, name: w.name })),
    ...(web ?? []).filter((w) => hit(w.name)).map((w) => ({ kind: "web" as const, id: w.id, name: w.name })),
  ];
  const pick = (i: number) => {
    const it = items[i];
    if (!it) return;
    onClose();
    onPick({ kind: it.kind, id: it.id });
  };

  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") onClose();
    else if (e.key === "ArrowDown") setSel((s) => Math.min(s + 1, items.length - 1));
    else if (e.key === "ArrowUp") setSel((s) => Math.max(s - 1, 0));
    else if (e.key === "Enter") pick(sel);
    else return;
    e.preventDefault();
  };

  return (
    <div className="fixed inset-0 z-50 flex justify-center bg-neutral-900/20 pt-24" onMouseDown={onClose}>
      <div
        role="dialog"
        aria-label={t("header.search")}
        className="flex h-fit max-h-[60vh] w-[560px] flex-col overflow-hidden rounded-xl border border-[#cde9f4] bg-white shadow-[0_20px_60px_-20px_rgba(41,83,124,.35)]"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <div className="flex items-center gap-2.5 border-b border-neutral-200 px-4">
          <Search className="size-4 text-main-deep-blue" aria-hidden="true" />
          <input
            autoFocus
            role="combobox"
            aria-expanded="true"
            aria-controls="search-results"
            className="h-12 flex-1 bg-transparent typo-body2_regular text-neutral-800 outline-none placeholder:text-neutral-400"
            placeholder={t("header.searchPlaceholder")}
            value={q}
            onChange={(e) => {
              setQ(e.target.value);
              setSel(0);
            }}
            onKeyDown={onKey}
          />
          {web === null && (
            <span className="inline-flex items-center gap-1 rounded-full border border-neutral-300 px-2 py-0.5 typo-caption2_semibold text-neutral-500">
              <WifiOff className="size-3" aria-hidden="true" />
              {t("header.webOffline")}
            </span>
          )}
        </div>
        <ul id="search-results" role="listbox" className="overflow-y-auto p-1.5">
          {items.length === 0 ? (
            <li className="px-3 py-6 text-center typo-body3_regular text-neutral-500">{t("header.searchEmpty")}</li>
          ) : (
            items.map((it, i) => {
              const Icon = it.kind === "local" ? Laptop : Globe;
              return (
                <li
                  key={`${it.kind}:${it.id}`}
                  role="option"
                  aria-selected={i === sel}
                  className={`flex cursor-pointer items-center gap-2.5 rounded-lg px-3 py-2 typo-body2_regular ${
                    i === sel ? "bg-main-light-blue text-main-deep-blue" : "text-neutral-800"
                  }`}
                  onMouseEnter={() => setSel(i)}
                  onClick={() => pick(i)}
                >
                  <Icon className="size-4 shrink-0 text-main-blue" aria-hidden="true" />
                  <span className="min-w-0 flex-1 truncate">{it.name}</span>
                  <span className="typo-caption1_medium text-neutral-400">{t(`dashboard.source.${it.kind}`)}</span>
                </li>
              );
            })
          )}
        </ul>
      </div>
    </div>
  );
}
