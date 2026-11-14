// 상태 줄 팝오버와 설정 › 통계 및 사용량이 같이 쓰는 구독 창 부품.
import { Check, ChevronDown } from "lucide-react";
import { useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import type { CredentialsView, PlanUsage, PlanWindow } from "../api";
import { t } from "../i18n";
import { credLabel } from "../screens/settings/AiPane";
import { formatReset } from "./format";

/** 사용 비율 0~100 정수. */
export const usedOf = (w: PlanWindow) => Math.max(0, Math.min(100, Math.round(w.usedPct)));
/** 가장 빡빡한(많이 쓴) 창. 연결 전·창 없음이면 undefined. */
export const tightest = (p: PlanUsage | null | undefined) =>
  p?.state === "ok" ? p.windows.reduce<PlanWindow | undefined>((a, w) => (!a || w.usedPct > a.usedPct ? w : a), undefined) : undefined;
/** 80% 넘으면 빨강 — 숫자가 같이 있어 색만으로 전하지 않는다. */
export const hot = (pct: number) => (pct > 80 ? "text-danger-700" : "");
export const usedText = (pct: number) => t("usage.used").replace("{pct}", String(pct));
export const resetText = (w: PlanWindow, now: Date) => t("usage.resetsIn").replace("{time}", formatReset(w.resetsAt, now));
/** 창 이름 — 백엔드가 준 label(모델·그룹), 없으면 kind 번역. */
export const windowName = (w: PlanWindow) => w.label ?? t(`usage.window.${w.kind}`);

/** 창 하나 — 이름·"N% 사용"·막대·초기화. `inline`이면 초기화를 오른쪽 글에 붙인다. */
export function WindowMeter({ w, now, inline = false }: { w: PlanWindow; now: Date; inline?: boolean }) {
  const pct = usedOf(w);
  const name = windowName(w);
  return (
    <div>
      <div className="flex justify-between gap-2 typo-caption1_medium text-neutral-700">
        <span>{name}</span>
        <span className={`tabular-nums ${hot(pct) || "text-neutral-500"}`}>{inline ? `${usedText(pct)} · ${resetText(w, now)}` : usedText(pct)}</span>
      </div>
      <div className="mt-1 h-1.5 overflow-hidden rounded-full bg-main-light-blue" role="progressbar" aria-label={name} aria-valuenow={pct} aria-valuemin={0} aria-valuemax={100}>
        <div className={`h-full rounded-full ${pct > 80 ? "bg-danger-600" : "bg-main-blue"}`} style={{ width: `${pct}%` }} />
      </div>
      {!inline && <div className="mt-1 typo-caption2_regular text-neutral-400">{resetText(w, now)}</div>}
    </div>
  );
}

export type DropItem<T extends string> = { id: T; name: string; icon: ReactNode; note?: ReactNode };

/** 오른쪽 위 드롭다운 — 고른 항목 [아이콘 이름 덧말 ⌄], 메뉴에 항목 목록(고른 것에 체크).
 *  바깥 클릭·Esc로 닫고, 열리면 고른 항목에 포커스, ↑↓로 옮기고 Enter·Space로 고른다. */
export function Dropdown<T extends string>({ label, items, sel, onSel }: { label: string; items: DropItem<T>[]; sel: T; onSel: (id: T) => void }) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const list = useRef<HTMLUListElement>(null);
  useEffect(() => {
    if (!open) return;
    list.current?.querySelector<HTMLElement>('[aria-selected="true"]')?.focus();
    const onDown = (e: MouseEvent) => !root.current?.contains(e.target as Node) && setOpen(false);
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [open]);
  const cur = items.find((i) => i.id === sel) ?? items[0];
  const choose = (id: T) => {
    onSel(id);
    setOpen(false);
    button.current?.focus();
  };
  const onKey = (e: KeyboardEvent) => {
    const opts = [...(list.current?.querySelectorAll<HTMLElement>('[role="option"]') ?? [])];
    const i = opts.indexOf(document.activeElement as HTMLElement);
    if (e.key === "Escape") {
      e.preventDefault();
      setOpen(false);
      button.current?.focus();
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      opts[e.key === "ArrowDown" ? Math.min(opts.length - 1, i + 1) : Math.max(0, i - 1)]?.focus();
    }
  };
  return (
    <div ref={root} className="relative shrink-0">
      <button
        ref={button}
        type="button"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={`${label}: ${cur.name}`}
        onClick={() => setOpen((o) => !o)}
        className="inline-flex h-8 items-center gap-2 rounded-lg border border-[#cde9f4] bg-white px-2.5 typo-caption1_semibold whitespace-nowrap text-neutral-700 hover:bg-main-light-blue/50"
      >
        {cur.icon}
        {cur.name}
        {cur.note}
        <ChevronDown className="size-3.5 text-neutral-400" aria-hidden="true" />
      </button>
      {open && (
        <ul
          ref={list}
          role="listbox"
          aria-label={label}
          onKeyDown={onKey}
          className="absolute right-0 z-20 mt-1 w-max min-w-44 rounded-lg border border-[#cde9f4] bg-white p-1 shadow-[0_12px_32px_-12px_rgba(41,83,124,.35)]"
        >
          {items.map((i) => (
            <li
              key={i.id}
              role="option"
              aria-selected={i.id === sel}
              tabIndex={0}
              onClick={() => choose(i.id)}
              onKeyDown={(e) => {
                if (e.key !== "Enter" && e.key !== " ") return;
                e.preventDefault();
                choose(i.id);
              }}
              className="flex h-8 cursor-pointer items-center gap-2 rounded-md px-2 typo-caption1_semibold whitespace-nowrap text-neutral-700 outline-none hover:bg-main-light-blue focus:bg-main-light-blue"
            >
              {i.icon}
              <span className="flex-1">{i.name}</span>
              {i.note}
              <Check className={`size-3.5 text-main-blue ${i.id === sel ? "" : "invisible"}`} aria-hidden="true" />
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/** 기록의 키·계정 id → 화면 이름표. 목록에 없으면(지운 키) id 그대로. */
export function credName(view: CredentialsView | null | undefined, id: string) {
  const all = view ? [...Object.values(view.apiKeys).flat(), ...view.claudeAccounts, ...view.codexAccounts] : [];
  const m = all.find((x) => x.id === id);
  if (m) return credLabel(m);
  if (id === "system") return t("settings.systemLabel");
  if (id === "default") return t("settings.keyDefaultLabel");
  return id;
}
