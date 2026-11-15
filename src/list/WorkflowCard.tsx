import { Boxes, FolderOpen, History, Rocket, Webhook, Zap, type LucideIcon } from "lucide-react";
import type { ReactNode } from "react";
import type { WebWorkflowSummary, WorkflowEntry } from "../api";
import { t } from "../i18n";
import { ago } from "../shell/Sidebar";
import { MoreMenu, type MoreItem } from "./MoreMenu";
import { ServiceIcons } from "./ServiceIcons";

export type CardStatus = "active" | "paused" | "error";
export type Trigger = "watch" | "manual" | "schedule" | "webhook" | "event";
export type CardData = {
  id: string;
  name: string;
  source: "local" | "web";
  status: CardStatus;
  services: string[];
  nodeCount: number | null;
  trigger: Trigger | null;
  /** 트리거 옆 덧붙임(감시 폴더). */
  triggerDetail?: string;
  /** 상태 알약에 마우스를 올리면 보이는 설명(감시 오류). */
  statusHint?: string;
  updatedAt: string | null;
};

const FOLDER_TRIGGER = "local.folder_new_file";

export function localCard(w: WorkflowEntry): CardData {
  const nodes = w.workflow.nodes;
  const watch = nodes.find((n) => n.event === FOLDER_TRIGGER);
  // 서비스 = 노드 event 앱(local.ai만 AI로 따로). 분기 같은 흐름 노드는 서비스가 아니다.
  const services = [...new Set(nodes.map((n) => (n.event === "local.ai" ? "ai" : n.event.split(".")[0])))].filter((s) => s === "local" || s === "ai");
  return {
    id: w.id,
    name: w.name,
    source: "local",
    status: w.enabled ? (w.watch === "watch_error" ? "error" : "active") : "paused",
    services,
    nodeCount: nodes.length,
    trigger: watch ? "watch" : "manual",
    triggerDetail: typeof watch?.config.folder === "string" && watch.config.folder ? watch.config.folder : undefined,
    statusHint: w.watchMessage ?? undefined,
    updatedAt: w.updatedAt,
  };
}

/** BE `TriggerType` → 카드 트리거. Gmail·GitHub 같은 앱 트리거는 "앱 이벤트". */
const webTrigger = (tt?: string | null): Trigger | null =>
  !tt ? null : tt === "MANUAL" ? "manual" : tt === "SCHEDULE" ? "schedule" : tt === "WEBHOOK" ? "webhook" : "event";

export function webCard(w: WebWorkflowSummary): CardData {
  return {
    id: w.id,
    name: w.name,
    source: "web",
    status: w.active ? "active" : "paused",
    services: w.services ?? [],
    nodeCount: w.nodeCount ?? null,
    trigger: webTrigger(w.triggerType),
    updatedAt: w.updatedAt ?? null,
  };
}

// 웹 WORKFLOW_STATUS_META 그대로.
export const STATUS_META: Record<CardStatus, { tone: string; bar: string; dot: string }> = {
  active: { tone: "border-node-green/15 bg-node-green/10 text-node-green", bar: "bg-[#006A4E]", dot: "bg-[#006A4E] shadow-[0_0_0_3px_#006A4E22]" },
  paused: { tone: "border-neutral-200 bg-neutral-100 text-neutral-500", bar: "bg-[#959595]", dot: "bg-[#959595]" },
  error: { tone: "border-danger-200 bg-danger-100 text-danger-700", bar: "bg-[#EC2D30]", dot: "bg-[#EC2D30]" },
};

const TRIGGER_ICON: Record<Trigger, LucideIcon> = { watch: FolderOpen, manual: Rocket, schedule: History, webhook: Webhook, event: Zap };

export function WorkflowCard({ data, onOpen, menu, children }: { data: CardData; onOpen(): void; menu: MoreItem[]; children?: ReactNode }) {
  const meta = STATUS_META[data.status];
  const TriggerIcon = data.trigger && TRIGGER_ICON[data.trigger];
  return (
    <article className="group relative flex min-h-[148px] flex-col rounded-xl border border-neutral-200 bg-white p-4 shadow-[0_1px_2px_rgba(16,24,40,.04)] hover:border-main-blue/40">
      <span className={`absolute inset-y-0 left-0 w-1 rounded-l-xl ${meta.bar}`} />
      {/* 카드 전체가 여는 버튼 — 내용은 그 위에 얹히되 메뉴만 따로 눌린다. */}
      <button type="button" onClick={onOpen} aria-label={data.name} className="absolute inset-0 z-10 rounded-xl" />
      <div className="pointer-events-none relative z-20 flex flex-1 flex-col gap-3">
        <div className="flex items-start gap-3 pl-1">
          <div className="min-w-0 flex-1">
            <h3 className="truncate typo-body1_semibold text-neutral-800">{data.name}</h3>
            {data.services.length > 0 && <ServiceIcons services={data.services} />}
          </div>
          <div className="pointer-events-auto">
            <MoreMenu items={menu} />
          </div>
        </div>
        <div className="mt-auto flex flex-col gap-3">
          {(data.nodeCount !== null || TriggerIcon) && (
            <div className="flex items-center gap-3 pl-1">
              {data.nodeCount !== null && (
                <span className="inline-flex shrink-0 items-center gap-1.5 typo-caption1_regular text-neutral-400">
                  <Boxes className="size-3.5" aria-hidden="true" />
                  {t("list.nodes").replace("{n}", String(data.nodeCount))}
                </span>
              )}
              {data.nodeCount !== null && TriggerIcon && <span className="h-3 w-px bg-neutral-200" />}
              {TriggerIcon && (
                <span className="inline-flex min-w-0 items-center gap-1.5 truncate typo-caption1_regular text-neutral-500">
                  <TriggerIcon className="size-3.5 shrink-0" aria-hidden="true" />
                  {t(`list.trigger.${data.trigger!}`)}
                  {data.triggerDetail && ` · ${data.triggerDetail}`}
                </span>
              )}
            </div>
          )}
          <div className="flex flex-wrap items-center gap-2 border-t border-neutral-200 pt-3">
            {data.updatedAt && <span className="typo-caption1_regular text-neutral-400">{t("list.edited").replace("{t}", ago(data.updatedAt))}</span>}
            <span className="flex-1" />
            <span className={`inline-flex h-7 items-center gap-1.5 rounded-full border px-2.5 typo-caption1_semibold ${meta.tone}`} title={data.statusHint}>
              <span className={`inline-block h-[7px] w-[7px] shrink-0 rounded-full ${meta.dot}`} />
              {t(`list.status.${data.status}`)}
            </span>
          </div>
          {children && <div className="pointer-events-auto">{children}</div>}
        </div>
      </div>
    </article>
  );
}
