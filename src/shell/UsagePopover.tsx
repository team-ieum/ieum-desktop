import { Check, ChevronRight, KeyRound, RefreshCw } from "lucide-react";
import { useEffect, useRef, useState, type KeyboardEvent, type MouseEvent, type Ref } from "react";
import { api, type CredentialsView, type PlanUsage, type UsageSummary } from "../api";
import { ProviderLogo } from "../assets/providers";
import { formatRelative } from "../dashboard/format";
import { t } from "../i18n";
import { accountsOf, credLabel, planKey, SUBS, usePlans, type Plans, type SubId } from "../screens/settings/AiPane";
import { useCredentials } from "../stores/credentials";
import { useDefaultPlans } from "../stores/plans";
import { formatReset, formatUsd } from "../usage/format";
import { hot, tightest, usedOf, WindowMeter } from "../usage/parts";
import { SwitchAccountDialog } from "../usage/SwitchAccount";

type Sub = (typeof SUBS)[number];

/** 구독 회사마다 기본 계정 사용량 — 연결된(창을 읽은) 것만. */
export function connectedSubs(view: CredentialsView | null | undefined, plans: Plans) {
  return SUBS.flatMap((s) => {
    const def = s.id === "antigravity" ? undefined : accountsOf(view, s.id).find((m) => m.isDefault);
    if (s.id !== "antigravity" && !def) return [];
    const plan = plans[planKey(s.id, def?.id)];
    const w = tightest(plan);
    return w && plan ? [{ sub: s, plan, w }] : [];
  });
}

/** 그 회사 계정마다 [이름, 기본, 상태]. Antigravity는 지금 로그인 하나. */
function accounts(s: Sub, view: CredentialsView | null | undefined, plans: Plans) {
  if (s.id === "antigravity") {
    const plan = plans[planKey("antigravity")];
    return [{ key: "antigravity", name: plan?.accountEmail ?? s.name, isDefault: true, plan }];
  }
  return accountsOf(view, s.id).map((m) => ({ key: m.id, name: m.email ?? credLabel(m), isDefault: m.isDefault, plan: plans[planKey(s.id, m.id)] }));
}

const PANEL = "rounded-brand-md border border-[#cde9f4] bg-white p-3 text-neutral-800 shadow-[0_12px_32px_-12px_rgba(41,83,124,.35)]";
const ICON_BTN = "grid size-7 place-items-center rounded-lg text-neutral-500 hover:bg-neutral-100";
const LINK = "flex h-8 w-full items-center justify-between rounded-lg px-2 typo-caption1_semibold text-main-deep-blue hover:bg-main-light-blue";

type Pick = { kind: "claude" | "codex"; fromName: string; to: { id: string; name: string; usedPct: number | null } };

/** 두 번째 패널 — 기본 계정의 창별 막대, 그 회사 계정 목록(체크 = 기본, 다른 계정을 누르면 바꾸기 확인), 계정 관리. */
function Flyout({ s, plan, view, plans, manage, onSettings, onPick, onEnter }: { s: Sub; plan: PlanUsage; view: CredentialsView | null | undefined; plans: Plans; manage: Ref<HTMLButtonElement>; onSettings: () => void; onPick: (p: Pick, from: HTMLButtonElement) => void; onEnter: () => void }) {
  const now = new Date();
  const list = accounts(s, view, plans);
  const listName = t("usage.accountsOf").replace("{name}", s.name);
  const fromName = list.find((a) => a.isDefault)?.name ?? "";
  return (
    <section aria-label={s.name} onMouseEnter={onEnter} className={`absolute bottom-0 left-full ml-2 w-80 ${PANEL}`}>
      <div className="mb-2 flex items-center gap-2">
        <ProviderLogo id={s.logo} size={18} />
        <span className="typo-body3_semibold text-main-deep-blue">{s.name}</span>
        {plan.checkedAt && <span className="ml-auto typo-caption2_regular text-neutral-400">{t("usage.updated").replace("{time}", formatRelative(plan.checkedAt))}</span>}
      </div>
      <div className="flex flex-col gap-3 rounded-lg bg-main-light-blue/40 p-3">
        {plan.windows.map((w) => (
          <WindowMeter key={`${w.kind}:${w.label ?? ""}`} w={w} now={now} />
        ))}
      </div>
      <div className="mt-3 mb-1 px-1 typo-caption2_semibold text-main-gray">{listName}</div>
      <ul aria-label={listName} className="flex flex-col">
        {list.map((a) => {
          const w = tightest(a.plan);
          const pct = w ? usedOf(w) : null;
          const pick = a.isDefault || s.id === "antigravity" ? undefined : (e: MouseEvent<HTMLButtonElement>) => onPick({ kind: s.id as Pick["kind"], fromName, to: { id: a.key, name: a.name, usedPct: pct } }, e.currentTarget);
          return (
            <li key={a.key} aria-label={a.name}>
              <button
                type="button"
                aria-disabled={pick ? undefined : "true"}
                onClick={pick}
                className={`flex h-7 w-full items-center gap-2 rounded-lg px-1 text-left typo-caption1_medium ${pick ? "hover:bg-main-light-blue/60" : "cursor-default"}`}
              >
                {a.isDefault ? <Check className="size-3.5 shrink-0 text-main-blue" aria-label={t("settings.defaultBadge")} /> : <span className="size-3.5 shrink-0" />}
                <span className="min-w-0 flex-1 truncate text-neutral-700">{a.name}</span>
                {pct !== null && <span className={`tabular-nums ${hot(pct) || "text-neutral-500"}`}>{pct}%</span>}
              </button>
            </li>
          );
        })}
      </ul>
      <div className="mt-2 border-t border-neutral-100 pt-2">
        <button ref={manage} type="button" className={LINK} onClick={onSettings}>
          {t("usage.manageAccounts")}
        </button>
      </div>
    </section>
  );
}

/** 상태 줄 사용량 팝오버 — 구독 회사 줄(남은 시간·사용 %), API 이번 달, 세부 정보·계정 관리 링크. 줄에 올리거나 →면 두 번째 패널. */
export function UsagePopover({ onUsage, onSettings }: { onUsage: () => void; onSettings: () => void }) {
  const view = useCredentials((s) => s.view);
  const { plans, refresh } = usePlans(view);
  const { refreshNow } = useDefaultPlans(view);
  // undefined = 읽는 중, null = 못 읽음
  const [month, setMonth] = useState<UsageSummary | null | undefined>(undefined);
  const [fly, setFly] = useState<SubId | null>(null);
  const [pick, setPick] = useState<Pick | null>(null);
  const pickFrom = useRef<HTMLButtonElement | null>(null);
  const [switched, setSwitched] = useState(0);
  // 바꾼 뒤 새 기본 계정 기준으로 사용량을 새로 읽는다(자격 목록이 바뀐 다음 렌더에서).
  useEffect(() => {
    if (switched) refreshNow();
  }, [switched]);
  // 패널이 열린 채 다른 줄을 스치면 150ms 뒤 바꾼다 — 그 사이 패널에 들어가면 취소(대각선 이동).
  const hoverT = useRef<ReturnType<typeof setTimeout>>(undefined);
  const stopHover = () => clearTimeout(hoverT.current);
  useEffect(() => stopHover, []);
  const hoverRow = (id: SubId) => {
    stopHover();
    if (!fly) setFly(id);
    else if (fly !== id) hoverT.current = setTimeout(() => setFly(id), 150);
  };
  const first = useRef<HTMLButtonElement>(null);
  const rows = useRef<Partial<Record<SubId, HTMLButtonElement | null>>>({});
  const manage = useRef<HTMLButtonElement>(null);
  const flyByKey = useRef(false);
  const loadMonth = () => api.usageSummary("month").then(setMonth, () => setMonth(null));
  useEffect(() => {
    void loadMonth();
    first.current?.focus();
  }, []);
  useEffect(() => {
    if (fly && flyByKey.current) manage.current?.focus();
    flyByKey.current = false;
  }, [fly]);

  const now = new Date();
  const subs = connectedSubs(view, plans);
  const open = subs.find((x) => x.sub.id === fly);

  const onKey = (e: KeyboardEvent) => {
    const list = subs.map((x) => rows.current[x.sub.id]).filter((b): b is HTMLButtonElement => !!b);
    const i = list.indexOf(document.activeElement as HTMLButtonElement);
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const next = e.key === "ArrowDown" ? (i < 0 ? 0 : Math.min(list.length - 1, i + 1)) : Math.max(0, i - 1);
      list[next]?.focus();
    } else if (e.key === "ArrowRight" && i >= 0) {
      e.preventDefault();
      stopHover();
      flyByKey.current = true;
      setFly(subs[i].sub.id);
    } else if (e.key === "ArrowLeft" && fly) {
      e.preventDefault();
      rows.current[fly]?.focus();
      setFly(null);
    }
  };

  return (
    <div role="dialog" aria-label={t("usage.popTitle")} onKeyDown={onKey} className={`absolute bottom-8 left-2 z-50 w-80 ${PANEL}`}>
      <div className="mb-1 flex items-center justify-between">
        <span className="typo-body3_semibold text-main-deep-blue">{t("usage.popTitle")}</span>
        <button
          ref={first}
          type="button"
          aria-label={t("usage.refresh")}
          title={t("usage.refresh")}
          className={ICON_BTN}
          onClick={() => {
            refresh();
            void loadMonth();
          }}
        >
          <RefreshCw className="size-3.5" aria-hidden="true" />
        </button>
      </div>
      <ul className="border-y border-neutral-100 py-1">
        {subs.map(({ sub: s, w }) => {
          const pct = usedOf(w);
          return (
            <li key={s.id}>
              <button
                ref={(b) => {
                  rows.current[s.id] = b;
                }}
                type="button"
                aria-expanded={fly === s.id}
                onMouseEnter={() => hoverRow(s.id)}
                onClick={() => {
                  stopHover();
                  setFly(s.id);
                }}
                className={`flex h-8 w-full items-center gap-2 rounded-lg px-2 typo-caption1_medium hover:bg-main-light-blue/60 ${fly === s.id ? "bg-main-light-blue/60" : ""}`}
              >
                <ProviderLogo id={s.logo} size={16} />
                <span className="flex-1 text-left font-semibold text-neutral-800">{s.name}</span>
                <span className="tabular-nums text-neutral-500">{formatReset(w.resetsAt, now)}</span>
                <span className={`w-9 text-right tabular-nums font-semibold ${hot(pct) || "text-neutral-800"}`}>{pct}%</span>
                <ChevronRight className="size-3.5 text-neutral-400" aria-hidden="true" />
              </button>
            </li>
          );
        })}
        <li className="flex h-8 items-center gap-2 px-2 typo-caption1_medium text-neutral-600">
          <KeyRound className="size-4 text-main-gray" aria-hidden="true" />
          <span className="tabular-nums">{month === undefined ? t("common.loading") : t("usage.apiMonth").replace("{usd}", formatUsd(month?.totalUsd ?? null))}</span>
        </li>
      </ul>
      <div className="mt-1 flex flex-col">
        <button type="button" className={LINK} onClick={onUsage}>
          {t("usage.detailsLink")}
          <ChevronRight className="size-3.5" aria-hidden="true" />
        </button>
        <button type="button" className={LINK} onClick={onSettings}>
          {t("usage.manageAccounts")}
          <ChevronRight className="size-3.5" aria-hidden="true" />
        </button>
      </div>
      {open && (
        <Flyout
          s={open.sub}
          plan={open.plan}
          view={view}
          plans={plans}
          manage={manage}
          onSettings={onSettings}
          onPick={(p, from) => {
            pickFrom.current = from;
            setPick(p);
          }}
          onEnter={stopHover}
        />
      )}
      {pick && (
        <SwitchAccountDialog
          {...pick}
          onClose={(ok) => {
            // 창을 연 계정 버튼으로 포커스를 돌린다(기본이 돼 aria-disabled여도 포커스는 받는다).
            pickFrom.current?.focus();
            setPick(null);
            if (ok) setSwitched((n) => n + 1);
          }}
        />
      )}
    </div>
  );
}
