import { ArrowLeft, CodeXml, History, LoaderCircle, Play, Save } from "lucide-react";
import { t } from "../i18n";

/** `</> 기술 정보` 스위치 — 켜면 노드 카드에 이벤트 id 등. 화면 상태로만 쥔다. */
export function TechSwitch({ on, onChange }: { on: boolean; onChange: (on: boolean) => void }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      aria-label={t("editor.techInfo")}
      onClick={() => onChange(!on)}
      className="inline-flex h-9 shrink-0 cursor-pointer items-center gap-2 rounded-[10px] px-2 text-sm font-semibold text-neutral-600 hover:bg-neutral-100"
    >
      <CodeXml size={16} aria-hidden="true" />
      <span aria-hidden="true">{t("editor.techInfo")}</span>
      <span className={`relative block h-5 w-9 shrink-0 rounded-full transition-colors ${on ? "bg-main-blue" : "bg-neutral-300"}`}>
        <span className={`absolute left-0 top-0.5 size-4 rounded-full bg-white shadow-sm transition-transform ${on ? "translate-x-4.5" : "translate-x-0.5"}`} />
      </span>
    </button>
  );
}

export interface ToolbarProps {
  name: string;
  onName: (name: string) => void;
  dirty: boolean;
  enabled: boolean;
  onEnabled: (enabled: boolean) => void;
  problems: number;
  onBack: () => void;
  /** 실행 기록(내 PC 실행 보기). 저장 전이면 없다. */
  onOpenRuns?: () => void;
  onSave: () => void;
  onRun: () => void;
  running: boolean;
  saving: boolean;
  techInfo: boolean;
  onTechInfo: (on: boolean) => void;
}

const ICON_BTN = "grid h-9 w-9 shrink-0 cursor-pointer place-items-center rounded-[10px] transition-colors disabled:cursor-not-allowed disabled:opacity-40";

/** 웹 WorkflowToolbar 클래스 — 공유 없이, Deploy 대신 실행. */
export function Toolbar({
  name,
  onName,
  dirty,
  enabled,
  onEnabled,
  problems,
  onBack,
  onOpenRuns,
  onSave,
  onRun,
  running,
  saving,
  techInfo,
  onTechInfo,
}: ToolbarProps) {
  const status = enabled ? t("list.status.active") : t("list.status.paused");
  return (
    <div className="flex h-16 shrink-0 items-center gap-3.5 overflow-x-auto border-b border-neutral-200 bg-white/85 px-6 backdrop-blur-[8px]">
      <button type="button" onClick={onBack} aria-label={t("common.back")} className={`${ICON_BTN} border border-neutral-200 bg-white text-neutral-600 hover:bg-neutral-50`}>
        <ArrowLeft size={18} aria-hidden="true" />
      </button>
      <div className="flex min-w-0 items-center gap-1.5">
        <input
          value={name}
          // size는 글자 수 기준 — 한글은 두 칸으로 센다.
          size={Math.max(4, [...name].reduce((n, c) => n + (c.charCodeAt(0) > 0xff ? 2 : 1), 0))}
          onChange={(e) => onName(e.target.value)}
          aria-label={t("editor.name")}
          className="min-w-0 rounded-md bg-transparent px-2 py-1 text-xl font-bold tracking-wide text-main-deep-blue outline-none transition-colors hover:bg-neutral-100 focus:bg-neutral-100"
        />
        {dirty && <span role="status" title={t("editor.unsaved")} aria-label={t("editor.unsaved")} className="size-2 shrink-0 rounded-full bg-amber-500" />}
      </div>
      <div className="flex shrink-0 items-center gap-2">
        <button
          type="button"
          role="switch"
          aria-checked={enabled}
          aria-label={t("list.status.active")}
          onClick={() => onEnabled(!enabled)}
          className={`relative h-6 w-11 cursor-pointer select-none rounded-full transition-colors duration-200 ${enabled ? "bg-node-green" : "bg-neutral-300"}`}
        >
          <span className={`absolute left-0 top-0.5 h-5 w-5 rounded-full bg-white shadow-sm transition-transform duration-200 ${enabled ? "translate-x-5" : "translate-x-0.5"}`} />
        </button>
        <span className={`text-xs font-semibold ${enabled ? "text-node-green" : "text-neutral-400"}`}>{status}</span>
      </div>
      {problems === 0 ? (
        <span className="inline-flex shrink-0 items-center gap-1.5 rounded-full border border-node-green/15 bg-node-green/10 px-2.5 py-1 text-xs font-semibold text-node-green">
          <span className="h-1.5 w-1.5 rounded-full bg-[#006A4E]" />
          {t("editor.noProblems")}
        </span>
      ) : (
        <span className="inline-flex shrink-0 items-center gap-1.5 rounded-full border border-danger-300 bg-danger-100 px-2.5 py-1 text-xs font-semibold text-danger-700">
          <span className="h-1.5 w-1.5 rounded-full bg-danger-700" />
          {t("editor.problems")} {problems}
        </span>
      )}
      <div className="flex-1" />
      <TechSwitch on={techInfo} onChange={onTechInfo} />
      <button
        type="button"
        onClick={onOpenRuns}
        disabled={!onOpenRuns}
        aria-label={t("run.title")}
        title={onOpenRuns ? t("run.title") : t("run.saveFirst")}
        className={`${ICON_BTN} text-neutral-500 hover:bg-neutral-100`}
      >
        <History size={18} aria-hidden="true" />
      </button>
      <button
        type="button"
        onClick={onSave}
        disabled={saving}
        className="inline-flex h-9 shrink-0 cursor-pointer items-center gap-2 rounded-[10px] border border-neutral-200 bg-white px-4 text-sm font-semibold text-neutral-700 transition-colors hover:bg-neutral-50 disabled:cursor-not-allowed disabled:opacity-60"
      >
        <Save size={15} aria-hidden="true" />
        {saving ? t("common.saving") : t("common.save")}
      </button>
      <button
        type="button"
        onClick={onRun}
        disabled={dirty || running}
        title={dirty ? t("run.saveFirst") : undefined}
        className="inline-flex h-9 shrink-0 cursor-pointer items-center gap-2 rounded-[10px] bg-main-deep-blue px-4 text-sm font-semibold text-white transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-60"
      >
        {running ? <LoaderCircle size={15} className="animate-spin motion-reduce:animate-none" aria-hidden="true" /> : <Play size={15} aria-hidden="true" />}
        {t("editor.run")}
      </button>
    </div>
  );
}
