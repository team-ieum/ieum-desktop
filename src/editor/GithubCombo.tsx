import { ChevronDown } from "lucide-react";
import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import type { OptionItem } from "../api";
import { formatRelative } from "../dashboard/format";
import { locale, t, tDyn, type Key } from "../i18n";
import { fieldKey, type Field } from "../nodes/defs";
import { opensIntegrations } from "../stores/runs";
import { optionsState, useGithubOptions } from "./githubOptions";

// 앱이 주는 덧말(한국어 이름표) → 화면 문구.
const HINT: Record<string, Key> = { "내 계정": "editor.gh.hint.me", 조직: "editor.gh.hint.org", 공개: "editor.gh.hint.public", 비공개: "editor.gh.hint.private" };
const hintText = (h: string) => (HINT[h] ? t(HINT[h]) : h);
// 30일부터는 달·해 단위 — 마지막 푸시가 오래된 저장소를 "731일 전"으로 보이지 않게.
export const agoText = (iso: string, now = Date.now()) => {
  const diff = Date.parse(iso) - now;
  const days = Math.abs(diff) / 86_400_000;
  if (days >= 30) {
    const rtf = new Intl.RelativeTimeFormat(locale(), { numeric: "always" });
    return days >= 365 ? rtf.format(Math.round(diff / (365 * 86_400_000)), "year") : rtf.format(Math.round(diff / (30 * 86_400_000)), "month");
  }
  return Math.abs(diff) < 60_000 ? t("editor.justNow") : formatRelative(iso, now, "always");
};

function Highlight({ text, q }: { text: string; q: string }) {
  const i = q ? text.toLowerCase().indexOf(q.toLowerCase()) : -1;
  if (i < 0) return <>{text}</>;
  return (
    <>
      {text.slice(0, i)}
      <mark className="rounded-[2px] bg-[#fff3b0] text-inherit">{text.slice(i, i + q.length)}</mark>
      {text.slice(i + q.length)}
    </>
  );
}

type Row = { item: OptionItem } | { asIs: true };

/** GitHub 목록 칸(시안 B) — 글 칸 그대로(친 글이 값) + 치면서 고르는 목록. 목록 밖 값·앞 단계 값도 그대로 쓴다. */
export function GithubCombo({
  id,
  event,
  field,
  value,
  config,
  fields,
  onChange,
  onOpenIntegrations,
}: {
  id: string;
  event: string;
  field: Field;
  value: string;
  config: Record<string, unknown>;
  fields: Field[];
  onChange: (v: string) => void;
  onOpenIntegrations?: () => void;
}) {
  const source = field.optionsSource!;
  const [open, setOpen] = useState(false);
  // 열고 나서 쳤을 때만 거른다 — 눌러 열면 전체 목록.
  const [filtering, setFiltering] = useState(false);
  const [active, setActive] = useState(0);
  const root = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const state = optionsState(field, config, fields);
  const { list, error, retry } = useGithubOptions(source, state, { open });

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => !root.current?.contains(e.target as Node) && setOpen(false);
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [open]);

  const q = filtering ? value : "";
  const items = (list?.items ?? []).filter((i) => !q || i.name.toLowerCase().includes(q.toLowerCase()) || i.id.toLowerCase().includes(q.toLowerCase()));
  const exact = list?.items.some((i) => i.id === value);
  const rows: Row[] = [...items.map((item) => ({ item })), ...(list && value && !exact ? [{ asIs: true as const }] : [])];
  const cur = Math.min(active, Math.max(0, rows.length - 1));
  const optId = (n: number) => `${id}-opt-${n}`;
  const showList = open && state.kind === "ready" && !error;

  // 키보드로 옮긴 줄이 목록 밖으로 나가지 않게 따라간다(jsdom엔 scrollIntoView가 없다).
  useEffect(() => {
    if (showList && list) document.getElementById(optId(cur))?.scrollIntoView?.({ block: "nearest" });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cur, showList, !!list]);

  const show = (filter: boolean) => {
    setOpen(true);
    setFiltering(filter);
    setActive(0);
  };
  const pick = (row: Row) => {
    if ("item" in row && row.item.id !== value) onChange(row.item.id);
    setOpen(false);
  };
  const onKey = (e: KeyboardEvent) => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (!open) show(false);
      else setActive(e.key === "ArrowDown" ? Math.max(0, Math.min(rows.length - 1, cur + 1)) : Math.max(0, cur - 1));
    } else if (e.key === "Enter" && showList && rows[cur]) {
      e.preventDefault();
      pick(rows[cur]);
    } else if (e.key === "Escape" && open) {
      e.preventDefault();
      e.stopPropagation(); // 패널까지 닫히지 않게
      setOpen(false);
    } else if (e.key === "Tab") setOpen(false);
  };

  // 칸 옆 칩: 내 계정, 마일스톤 제목.
  const match = list?.items.find((i) => i.id === value.trim());
  const chip = source === "github.owners" ? (match?.hint === "내 계정" ? t("editor.gh.hint.me") : null) : source === "github.milestones" ? (match?.name ?? null) : null;
  const head =
    state.kind === "ready" ? tDyn(`editor.gh.head.${source}`)?.replace("{owner}", state.inputs.owner ?? "").replace("{repo}", state.inputs.repo ?? "") : undefined;

  let blocked: string | null = null;
  if (state.kind === "blocked") {
    // 막은 의존 칸의 화면 이름(없으면 카탈로그 제목).
    const dep = field.optionsInputs?.find((k) => (fields.find((f) => f.key === k)?.title ?? k) === state.title);
    const title = (dep && tDyn(fieldKey(event, dep))) || state.title;
    blocked = t(`editor.gh.blocked.${state.reason}`).replace("{title}", title);
  }

  return (
    <div ref={root} className="flex flex-col gap-1.5">
      <div className="flex items-center gap-2 rounded-brand-sm border border-neutral-300 bg-white px-3 py-2 focus-within:border-main-deep-blue focus-within:ring-3 focus-within:ring-main-light-blue">
        <input
          ref={input}
          id={id}
          role="combobox"
          aria-autocomplete="list"
          aria-expanded={showList}
          aria-controls={showList ? `${id}-list` : undefined}
          aria-activedescendant={showList && rows[cur] ? optId(cur) : undefined}
          autoComplete="off"
          spellCheck={false}
          className={`min-w-0 flex-1 bg-transparent outline-none placeholder:text-neutral-500 ${
            value.includes("{{") ? "font-mono text-xs text-neutral-700" : "typo-body3_regular text-neutral-800"
          }`}
          placeholder={tDyn(`editor.gh.placeholder.${source}`)}
          value={value}
          onClick={() => !open && show(false)}
          onChange={(e) => {
            onChange(e.target.value);
            show(true);
          }}
          onKeyDown={onKey}
        />
        {chip && <span className="shrink-0 rounded-md bg-neutral-100 px-1.5 typo-caption2_regular text-neutral-600">{chip}</span>}
        <button
          type="button"
          tabIndex={-1}
          aria-label={t("editor.gh.toggle")}
          className="shrink-0 text-neutral-500"
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => {
            if (open) setOpen(false);
            else show(false);
            input.current?.focus();
          }}
        >
          <ChevronDown className={`size-3.5 ${open ? "rotate-180" : ""}`} aria-hidden="true" />
        </button>
      </div>
      {open && blocked && (
        <p role="note" className="rounded-brand-sm bg-main-light-blue px-2.5 py-2 typo-caption1_regular text-main-deep-blue">
          {blocked}
        </p>
      )}
      {open && error && (
        <p role="alert" className="rounded-brand-sm bg-danger-100 px-2.5 py-2 typo-caption1_regular text-danger-700">
          {opensIntegrations(error.code ?? undefined) ? t("editor.gh.notConnected") : error.message}{" "}
          {opensIntegrations(error.code ?? undefined) ? (
            onOpenIntegrations && (
              <button type="button" className="rounded-[7px] border border-neutral-300 bg-white px-2 py-0.5 typo-caption1_semibold text-neutral-700" onClick={onOpenIntegrations}>
                {t("editor.gh.openIntegrations")}
              </button>
            )
          ) : (
            <button type="button" className="rounded-[7px] border border-neutral-300 bg-white px-2 py-0.5 typo-caption1_semibold text-neutral-700" onClick={retry}>
              {t("settings.gh.retry")}
            </button>
          )}
        </p>
      )}
      {showList && (
        <ul
          id={`${id}-list`}
          role="listbox"
          aria-busy={!list}
          aria-label={!list ? t("editor.gh.loading") : undefined}
          className="mt-0.5 max-h-72 overflow-y-auto rounded-[10px] border border-neutral-200 bg-white p-1.5 typo-body3_regular text-neutral-800 shadow-[0_6px_18px_rgba(0,0,0,.10)]"
        >
          {!list ? (
            ["w-auto", "w-[70%]", "w-[55%]"].map((w) => <li key={w} role="presentation" data-testid="combo-skeleton" className={`mx-2.5 my-2 h-3 rounded-md bg-neutral-100 ${w}`} />)
          ) : (
            <>
              {head && (
                <li role="presentation" className="px-2.5 pt-1.5 pb-0.5 typo-caption2_semibold text-neutral-500">
                  {head}
                </li>
              )}
              {list.items.length === 0 && (
                <li role="presentation" className="px-2.5 py-[7px] text-neutral-500">
                  {t("editor.gh.empty")}
                </li>
              )}
              {rows.map((row, n) => (
                <li
                  key={"item" in row ? row.item.id : "\0asIs"}
                  id={optId(n)}
                  role="option"
                  aria-selected={n === cur}
                  className={`flex cursor-pointer items-center gap-2 rounded-[7px] px-2.5 py-[7px] ${n === cur ? "bg-main-light-blue font-semibold" : ""}`}
                  onMouseDown={(e) => e.preventDefault()}
                  onMouseEnter={() => setActive(n)}
                  onClick={() => pick(row)}
                >
                  {"item" in row ? (
                    <>
                      {source === "github.owners" && (
                        <span aria-hidden="true" className="grid size-[18px] shrink-0 place-items-center rounded-[5px] bg-neutral-800 text-[10px] text-white">
                          {row.item.name.charAt(0)}
                        </span>
                      )}
                      <span className="min-w-0 truncate">
                        <Highlight text={row.item.name} q={q} />
                      </span>
                      {(row.item.hint || row.item.updatedAt) && (
                        <span className="ml-auto shrink-0 typo-caption2_regular text-neutral-500">
                          {[row.item.hint && hintText(row.item.hint), row.item.updatedAt && agoText(row.item.updatedAt)].filter(Boolean).join(" · ")}
                        </span>
                      )}
                    </>
                  ) : (
                    t("editor.gh.useAsIs").replace("{text}", value)
                  )}
                </li>
              ))}
              {list.truncated && (
                <li role="presentation" className="mt-1 border-t border-neutral-200 px-2.5 pt-[7px] pb-[3px] typo-caption1_regular text-neutral-600">
                  {t("editor.gh.truncated")}
                </li>
              )}
            </>
          )}
        </ul>
      )}
    </div>
  );
}
