import { Bot, CircleDashed, FolderInput, FolderOpen, GitBranch, PenLine, Play, Trash2 } from "lucide-react";
import type { CSSProperties, ReactNode } from "react";
import { t } from "../i18n";
import { ServiceIcon, serviceLabel } from "../list/ServiceIcons";
import { DEFS, isLocal } from "./defs";

export type NodeMeta = { label: string; color: string; tint: string; icon: ReactNode };

export function NodeIcon({ event, size = 18 }: { event: string; size?: number }) {
  const Icon =
    {
      "local.folder_new_file": FolderOpen,
      "local.ai": Bot,
      "local.file_move": FolderInput,
      "local.file_rename": PenLine,
      "local.file_trash": Trash2,
      "branch.if": GitBranch,
    }[event] ?? CircleDashed;
  return <Icon size={size} aria-hidden="true" />;
}

const KIND_ICON = { TRIGGER: Play, AI: Bot, CONDITION: GitBranch } as const;
const KIND_COLOR = {
  TRIGGER: ["#2e8b68", "#dff2e9"],
  AI: ["#6d5ce7", "#eeeafd"],
  CONDITION: ["#9a6700", "#fff1d6"],
} as const;

/** 노드 종류 색·머리띠 문구·아이콘 — 시작 조건 초록, AI 보라, 조건 노랑, 내 PC 파일 작업 파랑, 웹 앱 노드 분홍. */
export function nodeMeta(event: string, type: string | undefined): NodeMeta {
  const kind = DEFS[event]?.node_type ?? type ?? "";
  if (kind in KIND_COLOR) {
    const k = kind as keyof typeof KIND_COLOR;
    const Icon = KIND_ICON[k];
    const [color, tint] = KIND_COLOR[k];
    return {
      label: t(`editor.kind.${k}`),
      color,
      tint,
      icon: isLocal(event) ? <NodeIcon event={event} size={20} /> : <Icon size={20} aria-hidden="true" />,
    };
  }
  if (isLocal(event))
    return { label: t("editor.kind.file"), color: "#287fa4", tint: "#e2f2f8", icon: <NodeIcon event={event} size={20} /> };
  // 웹 앱 노드 — 이벤트 앞부분이 서비스(`google_drive.upload` → GOOGLE_DRIVE).
  const service = event.split(".")[0].replace(/_bot$/, "").toUpperCase();
  return {
    label: serviceLabel(service),
    color: "#c75146",
    tint: "#fce8e5",
    icon: (
      <span className="grid size-6 place-items-center rounded-md bg-white">
        <ServiceIcon service={service} size="size-4" />
      </span>
    ),
  };
}

/** 노드에 CSS 변수로 색을 건다. */
export const toneStyle = (m: NodeMeta) => ({ "--node-color": m.color, "--node-tint": m.tint }) as CSSProperties;
