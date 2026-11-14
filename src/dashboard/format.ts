// 웹 WEB/utils/dashboard/mapWorkflowDashboardSummary.ts 이식.
import { locale } from "../i18n";

export function formatDuration(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return "—";
  if (seconds < 1) return `${Math.round(seconds * 1000)}ms`;
  if (seconds < 60) return `${seconds.toFixed(1)}s`;
  const m = Math.floor(seconds / 60);
  const r = Math.round(seconds % 60);
  return r > 0 ? `${m}m ${r}s` : `${m}m`;
}

/** `rate`는 0~100(BE·로컬 같은 단위). */
export const formatSuccessRate = (rate: number) => (Number.isFinite(rate) ? `${rate.toFixed(1)}%` : "—");

/** 00~23 24칸, 없는 시간은 0. 키는 "9"·"09" 둘 다 받는다. */
export const hourlySeries = (counts: Record<string, number>) =>
  Array.from({ length: 24 }, (_, h) => {
    const t = String(h).padStart(2, "0");
    return { t, count: counts[t] ?? counts[String(h)] ?? 0 };
  });

/** "방금"·"12분 전"·"3일 전" */
export function formatRelative(iso: string, now = Date.now()): string {
  const sec = Math.round((Date.parse(iso) - now) / 1000);
  const rtf = new Intl.RelativeTimeFormat(locale(), { numeric: "auto" });
  const a = Math.abs(sec);
  if (a < 60) return rtf.format(0, "second");
  if (a < 3600) return rtf.format(Math.round(sec / 60), "minute");
  if (a < 86400) return rtf.format(Math.round(sec / 3600), "hour");
  return rtf.format(Math.round(sec / 86400), "day");
}
