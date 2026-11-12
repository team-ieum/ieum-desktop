import { locale, t } from "../i18n";

/** 예상 금액 — null(가격표에 없음) "—", 0.01 미만 "<$0.01". */
export function formatUsd(n: number | null): string {
  if (n === null || !Number.isFinite(n)) return "—";
  if (n > 0 && n < 0.01) return "<$0.01";
  return `$${n.toFixed(2)}`;
}

/** ko "12.3만", en "123K". */
export const formatTokens = (n: number) => new Intl.NumberFormat(locale(), { notation: "compact", maximumFractionDigits: 1 }).format(n);

/** 초기화까지 남은 시간 — "2일 10시간"·"3시간 5분"·"35분". */
export function formatReset(iso: string, now: Date): string {
  const min = Math.max(0, Math.floor((Date.parse(iso) - now.getTime()) / 60_000));
  const d = Math.floor(min / 1440);
  const h = Math.floor((min % 1440) / 60);
  const m = min % 60;
  if (d) return `${d}${t("usage.day")} ${h}${t("usage.hour")}`;
  if (h) return `${h}${t("usage.hour")} ${m}${t("usage.minute")}`;
  return `${m}${t("usage.minute")}`;
}
