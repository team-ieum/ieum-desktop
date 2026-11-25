import { Bot, CircleDashed, FolderInput, FolderOpen, GitBranch, Globe2, PenLine, Play, Shuffle, Trash2 } from "lucide-react";
import type { CSSProperties, ReactNode } from "react";
import { t } from "../i18n";
import { ServiceIcon, serviceLabel } from "../list/ServiceIcons";
import { DEFS, isLocal } from "./defs";

/** `head`가 있으면 머리띠를 그 색으로 칠하고 흰 글자·흰 아이콘 상자로 그린다(서비스 브랜드 색). `shadow`는 카드 뒤 그림자 색(없으면 tint). */
export type NodeMeta = { label: string; color: string; tint: string; icon: ReactNode; head?: string; shadow?: string };

/** GitHub 마크 — simple-icons(https://simpleicons.org, CC0 1.0) 16.37.0 `github`의 path 그대로. 색은 글자색을 따른다. */
export function GithubMark({ size = 18 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <path d="M12 .297c-6.63 0-12 5.373-12 12 0 5.303 3.438 9.8 8.205 11.385.6.113.82-.258.82-.577 0-.285-.01-1.04-.015-2.04-3.338.724-4.042-1.61-4.042-1.61C4.422 18.07 3.633 17.7 3.633 17.7c-1.087-.744.084-.729.084-.729 1.205.084 1.838 1.236 1.838 1.236 1.07 1.835 2.809 1.305 3.495.998.108-.776.417-1.305.76-1.605-2.665-.3-5.466-1.332-5.466-5.93 0-1.31.465-2.38 1.235-3.22-.135-.303-.54-1.523.105-3.176 0 0 1.005-.322 3.3 1.23.96-.267 1.98-.399 3-.405 1.02.006 2.04.138 3 .405 2.28-1.552 3.285-1.23 3.285-1.23.645 1.653.24 2.873.12 3.176.765.84 1.23 1.91 1.23 3.22 0 4.61-2.805 5.625-5.475 5.92.42.36.81 1.096.81 2.22 0 1.606-.015 2.896-.015 3.286 0 .315.21.69.825.57C20.565 22.092 24 17.592 24 12.297c0-6.627-5.373-12-12-12" />
    </svg>
  );
}

export function NodeIcon({ event, size = 18 }: { event: string; size?: number }) {
  if (event.startsWith("github.")) return <GithubMark size={size} />;
  const Icon =
    {
      "local.folder_new_file": FolderOpen,
      "local.ai": Bot,
      "local.file_move": FolderInput,
      "local.file_rename": PenLine,
      "local.file_trash": Trash2,
      "branch.if": GitBranch,
    }[event] ??
    BUILTIN_ICON[DEFS[event]?.node_type as keyof typeof BUILTIN_ICON] ??
    CircleDashed;
  return <Icon size={size} aria-hidden="true" />;
}

// 이음 클라우드 편집기와 같은 색·아이콘.
const BUILTIN_ICON = { TRANSFORM: Shuffle, HTTP: Globe2 } as const;
const KIND_ICON = { TRIGGER: Play, AI: Bot, CONDITION: GitBranch, ...BUILTIN_ICON } as const;
const KIND_COLOR = {
  TRIGGER: ["#2e8b68", "#dff2e9"],
  AI: ["#6d5ce7", "#eeeafd"],
  CONDITION: ["#9a6700", "#fff1d6"],
  TRANSFORM: ["#287fa4", "#e2f2f8"],
  HTTP: ["#c75146", "#fce8e5"],
} as const;

/** 노드 종류 색·머리띠 문구·아이콘 — 시작 조건 초록, AI 보라, 조건 노랑, 내 PC 파일 작업·변환 파랑, 웹 앱 노드·HTTP 분홍. */
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
      icon: isLocal(event) || event.startsWith("github.") ? <NodeIcon event={event} size={20} /> : <Icon size={20} aria-hidden="true" />,
    };
  }
  if (isLocal(event))
    return { label: t("editor.kind.file"), color: "#287fa4", tint: "#e2f2f8", icon: <NodeIcon event={event} size={20} /> };
  // 웹 앱 노드 — 이벤트 앞부분이 서비스(`google_drive.upload` → GOOGLE_DRIVE).
  const service = event.split(".")[0].replace(/_bot$/, "").toUpperCase();
  // 브랜드 색이 있는 서비스는 그 색으로(이음 클라우드 편집기와 다름). 없으면 공통 분홍.
  if (service === "GITHUB")
    return { label: serviceLabel(service), ...GITHUB_TONE, icon: <GithubMark size={18} /> };
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

/** GitHub 헤더색 머리띠·GitHub 회색 바탕·테두리 회색 그림자. */
const GITHUB_TONE = { color: "#24292f", tint: "#f6f8fa", head: "#24292f", shadow: "#d0d7de" };

/** 노드에 CSS 변수로 색을 건다. 머리띠·아이콘 상자 변수는 `head`가 없으면 기본(tint 머리띠·color 아이콘 상자)과 같다. */
export const toneStyle = (m: NodeMeta) =>
  ({
    "--node-color": m.color,
    "--node-tint": m.tint,
    "--node-shadow": m.shadow ?? m.tint,
    "--node-head": m.head ?? m.tint,
    "--node-head-fg": m.head ? "#fff" : m.color,
    "--node-head-line": m.head ?? `color-mix(in srgb, ${m.color} 28%, white)`,
    "--node-icon-bg": m.head ? "#fff" : m.color,
    "--node-icon-fg": m.head ? m.head : "#fff",
    "--node-icon-shadow": m.head ? "none" : `0 4px 0 color-mix(in srgb, ${m.color} 78%, black)`,
  }) as CSSProperties;
