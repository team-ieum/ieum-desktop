import { Globe, Laptop, LogIn, Power, WifiOff, X } from "lucide-react";
import { useEffect, useState } from "react";
import { api, type WebDashboardExecution, type WebDashboardSummary, type WebError } from "../api";
import type { HeroSide } from "../dashboard/Hero";
import { Hero } from "../dashboard/Hero";
import { localSide, merge, webSide, type Source } from "../dashboard/metrics";
import { localRow, RunTable, webRow, type Row } from "../dashboard/RunTable";
import { StatusCards } from "../dashboard/StatusCards";
import { UsageCard } from "../dashboard/UsageCard";
import { t } from "../i18n";
import { useRuns } from "../stores/runs";
import { useSettings } from "../stores/settings";
import { useWeb } from "../stores/web";
import { useWorkflows } from "../stores/workflows";

// 첫 실행 자동 시작 안내 — 닫으면 다시 안 보인다(Rust 설정의 firstRunDone은 화면이 뜨기 전에 켜져 못 쓴다).
const BANNER_KEY = "ieum.autostartBannerClosed";
const bannerClosed = () => {
  try {
    return localStorage.getItem(BANNER_KEY) === "1";
  } catch {
    return false;
  }
};
const closeBanner = () => {
  try {
    localStorage.setItem(BANNER_KEY, "1");
  } catch {
    // 저장소를 못 쓰면 이번 실행에서만 닫힌다.
  }
};

const TABS: { id: Source; icon?: typeof Laptop }[] = [{ id: "all" }, { id: "local", icon: Laptop }, { id: "web", icon: Globe }];
const MAX_ROWS = 20;
// 연결 문제일 때만 "연결 안 됨" — 만료는 따로 [다시 로그인].
const offline = (e: unknown) => ["NETWORK", "SERVER"].includes((e as Partial<WebError> | null)?.code ?? "");

/** 내 PC 행 = 그 실행 상세. 웹 행 = 웹 워크플로우를 브라우저로. */
export function Dashboard({ onOpenRun }: { onOpenRun: (runId: string) => void }) {
  const [source, setSource] = useState<Source>("all");
  const { runs, live, load } = useRuns();
  const workflows = useWorkflows((s) => s.list);
  const loadWorkflows = useWorkflows((s) => s.load);
  const guard = useWeb((s) => s.guard);
  const expired = useWeb((s) => s.expired);
  const setRelogin = useWeb((s) => s.setRelogin);
  const autostart = useSettings((s) => s.settings?.autostart ?? false);
  const [banner, setBanner] = useState(() => !bannerClosed());
  const [summary, setSummary] = useState<WebDashboardSummary | null>(null);
  const [execs, setExecs] = useState<WebDashboardExecution[]>([]);
  const [webFail, setWebFail] = useState<unknown>(null);
  const webOff = webFail !== null && offline(webFail);

  useEffect(() => {
    void load();
    void loadWorkflows();
    // 실패하면(만료 포함) 웹 쪽만 비운다.
    guard(api.dashboardSummary()).then(setSummary, (e) => setWebFail(e ?? "failed"));
    guard(api.dashboardExecutions(null)).then(
      (p) => setExecs(p.content),
      () => setExecs([]),
    );
  }, [load, loadWorkflows, guard]);

  const local = localSide(runs, workflows, live, new Date());
  const web = summary ? webSide(summary) : null;
  const name = (id: string) => workflows.find((w) => w.id === id)?.name ?? t("history.deletedWorkflow");

  const sides: HeroSide[] =
    source === "all"
      ? [
          { label: t("dashboard.tab.local"), side: local },
          { label: t("dashboard.tab.web"), side: web },
        ]
      : source === "local"
        ? [{ label: t("dashboard.tab.local"), side: local }]
        : [{ label: t("dashboard.tab.web"), side: web }];
  const m = merge(source === "web" && web ? web : local, source === "all" ? web : null);

  const rows: Row[] = [
    ...(source !== "web" ? runs.map((r) => localRow(r, name(r.workflowId))) : []),
    ...(source !== "local" ? execs.map(webRow) : []),
  ]
    .sort((a, b) => Date.parse(b.at || "0") - Date.parse(a.at || "0"))
    .slice(0, MAX_ROWS);

  const open = (r: Row) =>
    r.source === "local"
      ? onOpenRun(r.id)
      : void api.openWebWorkflow(r.workflowId).catch((e) => console.error("[ieum] 웹 워크플로우를 열지 못했어요", e));
  // 탭 ←→ 이동(WAI-ARIA tabs)
  const onTabKey = (e: React.KeyboardEvent) => {
    const step = e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0;
    if (!step) return;
    e.preventDefault();
    const next = TABS[(TABS.findIndex((x) => x.id === source) + step + TABS.length) % TABS.length].id;
    setSource(next);
    document.getElementById(`dashboard-tab-${next}`)?.focus();
  };

  return (
    <section className="flex flex-col gap-3.5 px-6 py-6">
      <div className="flex items-end justify-between gap-4">
        <div>
          <h1 className="typo-title2_bold text-main-deep-blue">{t("dashboard.title")}</h1>
          <p className="typo-body2_regular mt-1 text-neutral-500">{t("dashboard.subtitle")}</p>
        </div>
        <div className="flex items-center gap-2">
          {expired && (
            <button
              type="button"
              className="inline-flex items-center gap-1.5 rounded-full border border-danger-300 bg-danger-100 px-2.5 py-1 typo-caption1_semibold text-danger-700 hover:brightness-95"
              title={t("settings.reloginHint")}
              onClick={() => setRelogin(true)}
            >
              <LogIn className="size-3" aria-hidden="true" />
              {t("settings.relogin")}
            </button>
          )}
          {!expired && webOff && (
            <span className="inline-flex items-center gap-1.5 rounded-full border border-neutral-300 bg-neutral-white px-2.5 py-1 typo-caption1_semibold text-neutral-600">
              <WifiOff className="size-3" aria-hidden="true" />
              {t("dashboard.webOffline")}
            </span>
          )}
          <div role="tablist" onKeyDown={onTabKey} className="inline-flex rounded-[10px] border border-neutral-200 bg-neutral-100 p-1">
            {TABS.map(({ id, icon: Icon }) => (
              <button
                key={id}
                type="button"
                role="tab"
                id={`dashboard-tab-${id}`}
                aria-selected={source === id}
                aria-controls="dashboard-panel"
                tabIndex={source === id ? 0 : -1}
                className={`inline-flex h-8 items-center gap-1.5 rounded-lg px-4 typo-body3_semibold ${
                  source === id ? "bg-main-deep-blue text-white shadow-[0_1px_2px_rgba(16,24,40,.08)]" : "text-neutral-600"
                }`}
                onClick={() => setSource(id)}
              >
                {Icon && <Icon className="size-3.5" aria-hidden="true" />}
                {t(`dashboard.tab.${id}`)}
              </button>
            ))}
          </div>
        </div>
      </div>

      {banner && autostart && (
        <div role="status" className="flex items-center gap-2.5 rounded-xl border border-[#cde9f4] bg-main-light-blue/50 px-4 py-2">
          <Power className="size-4 text-main-blue" aria-hidden="true" />
          <p className="flex-1 typo-body3_medium text-main-deep-blue">{t("banner.autostart")}</p>
          <button
            type="button"
            className="grid h-6 w-6 place-items-center rounded-md text-neutral-400 hover:bg-white"
            aria-label={t("banner.close")}
            onClick={() => {
              closeBanner();
              setBanner(false);
            }}
          >
            <X className="size-3.5" aria-hidden="true" />
          </button>
        </div>
      )}

      <div id="dashboard-panel" role="tabpanel" aria-labelledby={`dashboard-tab-${source}`} className="flex flex-col gap-3.5">
        {source === "web" && !web ? (
          <p className="rounded-brand-md border border-dashed border-neutral-300 bg-white p-8 text-center typo-body2_regular text-neutral-500">
            {webFail !== null ? t("dashboard.webOfflineBody") : t("common.loading")}
          </p>
        ) : (
          <>
            <Hero todayRuns={m.todayRuns} hourly={m.hourly} sides={sides} />
            <StatusCards stats={m.stats} />
            <UsageCard hidden={source === "web"} />
            <div className="overflow-hidden rounded-brand-md border border-neutral-200 bg-neutral-white shadow-sm">
              <div className="flex items-center justify-between gap-4 border-b border-neutral-200 px-5 py-3.5">
                <h3 className="typo-body2_semibold text-neutral-900">{t("dashboard.recent")}</h3>
              </div>
              <RunTable rows={rows} showSource={source === "all"} onOpen={open} />
            </div>
          </>
        )}
      </div>
    </section>
  );
}
