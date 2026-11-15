import { Bell, CircleX, Globe, Laptop } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { api, type WebDashboardError } from "../api";
import { formatRelative } from "../dashboard/format";
import { t } from "../i18n";
import { useRuns } from "../stores/runs";
import { useSettings } from "../stores/settings";
import { useWeb } from "../stores/web";
import { useWorkflows } from "../stores/workflows";

type Item = { kind: "local" | "web"; id: string; workflowId: string; name: string; message: string | null; at: string };
const MAX = 20;
/** 웹 오류 다시 읽는 간격 — 창이 보일 때만. */
export const POLL_MS = 60_000;

/** 위쪽 줄 종 — 내 PC 실패 + 웹 이음 오류. 배지 = 마지막으로 연 뒤 생긴 수. */
export function NotificationsMenu({ onOpenRun }: { onOpenRun: (runId: string) => void }) {
  const runs = useRuns((s) => s.runs);
  const loadRuns = useRuns((s) => s.load);
  const workflows = useWorkflows((s) => s.list);
  const settings = useSettings((s) => s.settings);
  const save = useSettings((s) => s.save);
  const guard = useWeb((s) => s.guard);
  const [webErrors, setWebErrors] = useState<WebDashboardError[]>([]);
  const [open, setOpen] = useState(false);

  const bell = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);

  // 내 PC 실패는 runFinished 때 subscribeExecEvents가 useRuns를 다시 읽어 따라온다.
  // 웹 오류는 창이 보이는 동안 60초마다, 다시 보일 때 바로.
  useEffect(() => {
    void loadRuns();
    const fetchWeb = () =>
      guard(api.dashboardErrors(null)).then(
        (p) => setWebErrors(p.content),
        () => setWebErrors([]),
      );
    void fetchWeb();
    const tick = setInterval(() => document.visibilityState === "visible" && void fetchWeb(), POLL_MS);
    const onVisible = () => document.visibilityState === "visible" && void fetchWeb();
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      clearInterval(tick);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, [loadRuns, guard]);

  // 열리면 첫 항목(없으면 메뉴)으로 포커스 — Esc가 먹게.
  useEffect(() => {
    if (open) (menu.current?.querySelector<HTMLElement>("[role=menuitem]") ?? menu.current)?.focus();
  }, [open]);

  const items: Item[] = [
    ...runs
      .filter((r) => r.status === "failed" && r.trigger !== "test")
      .map((r) => ({
        kind: "local" as const,
        id: r.runId,
        workflowId: r.workflowId,
        name: workflows.find((w) => w.id === r.workflowId)?.name ?? t("history.deletedWorkflow"),
        message: null,
        at: r.finishedAt || r.startedAt,
      })),
    ...webErrors.map((e) => ({
      kind: "web" as const,
      id: e.executionId,
      workflowId: e.workflowId,
      name: e.workflowName,
      message: e.errorMessage,
      at: e.startedAt ?? "",
    })),
  ]
    .sort((a, b) => Date.parse(b.at || "0") - Date.parse(a.at || "0"))
    .slice(0, MAX);

  const seen = Date.parse(settings?.notificationsSeenAt ?? "") || 0;
  const unread = items.filter((i) => Date.parse(i.at) > seen).length;

  const toggle = () => {
    if (!open && settings) void save({ ...settings, notificationsSeenAt: new Date().toISOString() }).catch(() => {});
    setOpen(!open);
  };
  const close = () => {
    setOpen(false);
    bell.current?.focus();
  };
  const onMenuKey = (e: React.KeyboardEvent) => {
    const els = [...(menu.current?.querySelectorAll<HTMLElement>("[role=menuitem]") ?? [])];
    const at = els.indexOf(document.activeElement as HTMLElement);
    const go = (i: number) => els[(i + els.length) % els.length]?.focus();
    if (e.key === "Escape") close();
    else if (e.key === "ArrowDown") go(at + 1);
    else if (e.key === "ArrowUp") go(at - 1);
    else if (e.key === "Home") go(0);
    else if (e.key === "End") go(els.length - 1);
    else if (e.key === "Tab") setOpen(false);
    else return;
    if (e.key !== "Tab") e.preventDefault();
  };
  const choose = (i: Item) => {
    setOpen(false);
    if (i.kind === "local") onOpenRun(i.id);
    else void api.openWebWorkflow(i.workflowId).catch(() => {});
  };

  return (
    <div className="relative">
      <button
        ref={bell}
        type="button"
        aria-label={t("header.notifications")}
        aria-haspopup="menu"
        aria-expanded={open}
        className="relative flex h-[38px] w-[38px] items-center justify-center rounded-xl border border-[#cde9f4] bg-white/60 text-main-deep-blue hover:bg-white"
        onClick={toggle}
      >
        <Bell className="size-[18px]" aria-hidden="true" />
        {unread > 0 && (
          <span
            data-testid="bell-badge"
            className="absolute top-1.5 right-1.5 flex h-[14px] min-w-[14px] items-center justify-center rounded-full border-2 border-[#EAF7FE] bg-node-orange px-0.5 text-[8px] font-bold text-white"
          >
            {unread}
          </span>
        )}
      </button>
      {open && (
        <>
          <div className="fixed inset-0 z-40" onClick={() => setOpen(false)} />
          <div
            ref={menu}
            role="menu"
            aria-label={t("header.notifications")}
            tabIndex={-1}
            onKeyDown={onMenuKey}
            className="absolute top-12 right-0 z-50 flex max-h-[420px] w-[340px] flex-col overflow-hidden rounded-xl border border-[#cde9f4] bg-white shadow-[0_20px_60px_-20px_rgba(41,83,124,.35)]"
          >
            <p className="border-b border-neutral-200 px-4 py-3 typo-body2_bold text-main-deep-blue">{t("header.notifications")}</p>
            {items.length === 0 ? (
              <p className="px-4 py-8 text-center typo-body3_regular text-neutral-500">{t("header.notificationsEmpty")}</p>
            ) : (
              <div className="overflow-y-auto p-1.5">
                {items.map((i) => {
                  const Src = i.kind === "local" ? Laptop : Globe;
                  return (
                    <button
                      key={`${i.kind}:${i.id}`}
                      type="button"
                      role="menuitem"
                      tabIndex={-1}
                      className="flex w-full items-start gap-2.5 rounded-lg px-3 py-2 text-left hover:bg-neutral-50"
                      onClick={() => choose(i)}
                    >
                      <CircleX className="mt-0.5 size-4 shrink-0 text-danger-600" aria-hidden="true" />
                      <span className="flex min-w-0 flex-1 flex-col">
                        <span className="truncate typo-body3_semibold text-neutral-800">{i.name}</span>
                        <span className="truncate typo-caption1_regular text-neutral-500">{i.message || t("header.runFailed")}</span>
                      </span>
                      <span className="flex shrink-0 flex-col items-end gap-0.5 typo-caption2_semibold text-neutral-400">
                        <Src className="size-3" aria-hidden="true" />
                        {i.at ? formatRelative(i.at) : ""}
                      </span>
                    </button>
                  );
                })}
              </div>
            )}
          </div>
        </>
      )}
    </div>
  );
}
