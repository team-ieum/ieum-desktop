import { Globe, Laptop, LogIn, Power, WifiOff, X } from "lucide-react";
import { useEffect, useState } from "react";
import { api, type WebDashboardExecution, type WebDashboardSummary, type WebError } from "../api";
import type { HeroSide } from "../dashboard/Hero";
import { Hero } from "../dashboard/Hero";
import { localSide, merge, webSide, type Source } from "../dashboard/metrics";
import {
  filterRows,
  localRow,
  NO_FILTERS,
  PAGE_SIZE,
  Pager,
  RunFilters,
  RunsNoMatch,
  RunTable,
  statusOptions,
  triggerOptions,
  webRow,
  type Filters,
  type Row,
} from "../dashboard/RunTable";
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
// 연결 문제일 때만 "연결 안 됨" — 만료는 따로 [다시 로그인].
const offline = (e: unknown) => ["NETWORK", "SERVER"].includes((e as Partial<WebError> | null)?.code ?? "");

/** 내 PC 행 = 그 실행 상세. 웹 행 = 웹 워크플로우를 브라우저로. */
export function Dashboard({ onOpenRun, onUsage, onAccounts }: { onOpenRun: (runId: string) => void; onUsage: () => void; onAccounts: () => void }) {
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
  /** 웹 다음 쪽 커서 — null이면 끝. */
  const [next, setNext] = useState<string | null>(null);
  const [fetching, setFetching] = useState(false);
  const [filters, setFilters] = useState<Filters>(NO_FILTERS);
  const [page, setPage] = useState(0);
  const [webFail, setWebFail] = useState<unknown>(null);
  const webOff = webFail !== null && offline(webFail);

  useEffect(() => {
    void load();
    void loadWorkflows();
    // 실패하면(만료 포함) 웹 쪽만 비운다.
    guard(api.dashboardSummary()).then(setSummary, (e) => setWebFail(e ?? "failed"));
    guard(api.dashboardExecutions(null)).then(
      (p) => {
        setExecs(p.content);
        setNext(p.hasNext ? p.nextCursor : null);
      },
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

  const time = (r: Row) => Date.parse(r.at || "0") || 0;
  const webRows = source !== "local" ? execs.map(webRow) : [];
  const all: Row[] = [...(source !== "web" ? runs.map((r) => localRow(r, name(r.workflowId))) : []), ...webRows].sort((a, b) => time(b) - time(a));
  const rows = filterRows(all, filters);
  const shown = rows.slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE);
  // BE는 상태·트리거 거르기를 안 받아 받은 것에서 거른다 — 이 쪽이 덜 찼거나, (전체 탭) 이 쪽 끝이 받은 웹보다 오래됐으면 다음 커서만 더 받는다.
  // ponytail: 드문 필터는 맞는 행이 찰 때까지 커서를 끝까지 따라간다 — 느리면 BE에 status·trigger 쿼리를 추가.
  const webOldest = webRows.length ? Math.min(...webRows.map(time)) : Infinity;
  const needMore = next !== null && !fetching && source !== "local" && (shown.length < PAGE_SIZE || time(shown[shown.length - 1]) < webOldest);
  useEffect(() => {
    if (!needMore || next === null) return;
    setFetching(true);
    guard(api.dashboardExecutions(next))
      .then(
        (p) => {
          setExecs((e) => [...e, ...p.content]);
          setNext(p.hasNext ? p.nextCursor : null);
        },
        () => setNext(null),
      )
      .finally(() => setFetching(false));
  }, [needMore, next, guard]);
  const pickSource = (id: Source) => {
    setSource(id);
    setPage(0);
  };
  const changeFilters = (f: Filters) => {
    setFilters(f);
    setPage(0);
  };

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
    pickSource(next);
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
                onClick={() => pickSource(id)}
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
            <UsageCard hidden={source === "web"} onUsage={onUsage} onAccounts={onAccounts} />
            <div className="overflow-hidden rounded-brand-md border border-neutral-200 bg-neutral-white shadow-sm">
              <div className="flex items-center justify-between gap-4 border-b border-neutral-200 px-5 py-3.5">
                <h3 className="typo-body2_semibold text-neutral-900">{t("dashboard.recent")}</h3>
                {all.length > 0 && (
                  <RunFilters value={filters} onChange={changeFilters} statuses={statusOptions(source !== "web")} triggers={triggerOptions(all)} />
                )}
              </div>
              {all.length > 0 && rows.length === 0 ? (
                fetching ? (
                  <p className="px-5 py-8 text-center typo-body2_regular text-neutral-500">{t("common.loading")}</p>
                ) : (
                  <RunsNoMatch onClear={() => changeFilters(NO_FILTERS)} />
                )
              ) : (
                <RunTable rows={shown} showSource={source === "all"} onOpen={open} />
              )}
              {rows.length > 0 && <Pager page={page} total={rows.length} more={next !== null && source !== "local"} onPage={setPage} />}
            </div>
          </>
        )}
      </div>
    </section>
  );
}
