// 상태 줄 팝오버와 설정 › 통계 및 사용량이 같이 쓰는 구독 창 부품.
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

/** 창 하나 — 이름·"N% 사용"·막대·초기화. `inline`이면 초기화를 오른쪽 글에 붙인다. */
export function WindowMeter({ w, now, inline = false }: { w: PlanWindow; now: Date; inline?: boolean }) {
  const pct = usedOf(w);
  const name = t(`usage.window.${w.kind}`);
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

/** 기록의 키·계정 id → 화면 이름표. 목록에 없으면(지운 키) id 그대로. */
export function credName(view: CredentialsView | null | undefined, id: string) {
  const all = view ? [...Object.values(view.apiKeys).flat(), ...view.claudeAccounts, ...view.codexAccounts] : [];
  const m = all.find((x) => x.id === id);
  if (m) return credLabel(m);
  if (id === "system") return t("settings.systemLabel");
  if (id === "default") return t("settings.keyDefaultLabel");
  return id;
}
