import { Bot, Laptop } from "lucide-react";
import { tDyn } from "../i18n";

// 웹 serviceIcon 12개 — 이름은 BE `WorkflowServiceType`에서 `_`를 빼고 소문자(GOOGLE_DRIVE → googledrive).
const PNG = import.meta.glob<string>("../assets/service/*.png", { eager: true, import: "default" });
const png = (s: string) => PNG[`../assets/service/${s.toLowerCase().replace(/_/g, "")}.png`];

const BRAND: Record<string, string> = {
  GMAIL: "Gmail",
  GOOGLE_DRIVE: "Google Drive",
  GOOGLE_SHEETS: "Google Sheets",
  NOTION: "Notion",
  GITHUB: "GitHub",
  SLACK: "Slack",
  DISCORD: "Discord",
  JIRA: "Jira",
  AIRTABLE: "Airtable",
  LINEAR: "Linear",
};

export const serviceLabel = (s: string) => tDyn(`list.svc.${s}`) ?? BRAND[s] ?? s;

/** 서비스 하나의 그림 — `local`·`ai`는 lucide, 웹 서비스는 PNG, 모르는 이름은 첫 글자. */
export function ServiceIcon({ service, size }: { service: string; size: string }) {
  if (service === "local") return <Laptop className={`${size} text-main-deep-blue`} aria-hidden="true" />;
  if (service === "ai") return <Bot className={`${size} text-node-purple`} aria-hidden="true" />;
  const src = png(service);
  if (src) return <img src={src} className={`${size} object-contain`} alt="" />;
  return <span className="typo-caption2_semibold text-neutral-500">{service.charAt(0)}</span>;
}

export function ServiceIcons({ services }: { services: string[] }) {
  return (
    <div className="mt-2.5 flex items-center gap-1.5">
      {services.map((s) => (
        <span key={s} className="grid h-7 w-7 shrink-0 place-items-center rounded-md border border-neutral-200 bg-white" title={serviceLabel(s)}>
          <ServiceIcon service={s} size="size-4" />
        </span>
      ))}
    </div>
  );
}
