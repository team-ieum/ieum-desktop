import { MoreHorizontal } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { t } from "../i18n";

export type MoreItem = { label: string; danger?: boolean; onSelect(): void };

/** 카드 오른쪽 위 "…" 메뉴. 바깥을 누르거나 Esc면 닫힌다. */
export function MoreMenu({ items }: { items: MoreItem[] }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const close = (e: Event) => {
      if (!(e instanceof KeyboardEvent ? e.key === "Escape" : !ref.current?.contains(e.target as Node))) return;
      setOpen(false);
    };
    document.addEventListener("mousedown", close);
    document.addEventListener("keydown", close);
    return () => {
      document.removeEventListener("mousedown", close);
      document.removeEventListener("keydown", close);
    };
  }, [open]);

  if (items.length === 0) return null;
  return (
    <div ref={ref} className="relative z-30">
      <button
        type="button"
        aria-label={t("list.menu")}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={(e) => {
          e.stopPropagation();
          setOpen((o) => !o);
        }}
        className={`grid h-8 w-8 shrink-0 place-items-center rounded-lg ${open ? "bg-neutral-100 text-neutral-600" : "text-neutral-400 hover:bg-neutral-100"}`}
      >
        <MoreHorizontal className="size-[17px]" aria-hidden="true" />
      </button>
      {open && (
        <div role="menu" className="absolute right-0 top-full z-50 mt-1 min-w-[140px] overflow-hidden rounded-lg border border-neutral-200 bg-white py-1 shadow-lg">
          {items.map((it) => (
            <button
              key={it.label}
              type="button"
              role="menuitem"
              onClick={(e) => {
                e.stopPropagation();
                setOpen(false);
                it.onSelect();
              }}
              className={`flex w-full items-center gap-2 px-3 py-2 text-left text-sm ${it.danger ? "text-danger-600 hover:bg-danger-50" : "text-neutral-700 hover:bg-neutral-50"}`}
            >
              {it.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
