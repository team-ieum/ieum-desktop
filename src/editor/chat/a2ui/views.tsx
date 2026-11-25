import { Check, ChevronRight, CircleCheck, CircleX, FileText, Folder, FolderOpen, FolderSearch, RefreshCw, ShieldCheck, Sparkles } from "lucide-react";
import { type ReactNode, useEffect, useId, useState } from "react";
import { t, tDyn } from "../../../i18n";
import { fieldKey } from "../../../nodes/defs";
import { NodeIcon } from "../../../nodes/meta";
import { CARD, CARD_BUTTON } from "../ChatCards";

const TAG = "inline-flex shrink-0 items-center gap-0.5 rounded-full px-1.5 py-0.5 text-[10px] font-bold";
const NAME = "font-semibold text-neutral-700";
const BOX = "rounded-lg border border-[#d4e4ec] bg-white px-2 py-1.5 text-xs text-neutral-900 disabled:bg-neutral-50 disabled:text-neutral-500";

/** 설계서 카드 바깥틀 — 제목과 자식 컴포넌트를 담는다. 글자는 LLM이 보낸 그대로. */
export function BlueprintView({
  title,
  status,
  buildPending,
  children,
}: {
  title: string;
  status: "active" | "submitted" | "cancelled";
  buildPending: boolean;
  children?: ReactNode;
}) {
  return (
    <section role="group" aria-label={title} className={`${CARD} max-w-none flex flex-col gap-2.5`}>
      <div className="flex flex-wrap items-center gap-1.5">
        <FileText size={13} className="shrink-0 text-main-deep-blue" aria-hidden="true" />
        <h3 className="text-xs font-semibold text-main-deep-blue">{title}</h3>
        <span className={`${TAG} ml-auto bg-[#f1efff] text-[#6d5bd0]`}>
          <Sparkles size={11} aria-hidden="true" />
          {t("chat.blueprint.aiMade")}
        </span>
        {status === "submitted" && !buildPending && (
          <span className={`${TAG} bg-[#e6f4ee] text-[#2f7d58]`}>
            <CircleCheck size={11} aria-hidden="true" />
            {t("chat.blueprint.built")}
          </span>
        )}
        {status === "cancelled" && (
          <span className={`${TAG} bg-neutral-100 text-neutral-600`}>
            <CircleX size={11} aria-hidden="true" />
            {t("chat.blueprint.cancelled")}
          </span>
        )}
      </div>
      {children}
    </section>
  );
}

/** 이번에 바뀐 컴포넌트를 2초 동안 노랗게 강조한다. */
export function Changed({ on, children }: { on: boolean; children: ReactNode }) {
  const [hot, setHot] = useState(on);
  useEffect(() => {
    if (!on) {
      setHot(false);
      return;
    }
    setHot(true);
    const id = setTimeout(() => setHot(false), 2000);
    return () => clearTimeout(id);
  }, [on]);
  return <div className={`rounded-lg transition-shadow duration-500 ${hot ? "bg-amber-50 shadow-[0_0_0_2px_#fbbf24]" : ""}`}>{children}</div>;
}

/** 흐름 미리보기 — 단계 칩. event가 있으면 그 노드 아이콘을 붙인다. */
export function FlowPreviewView({ steps }: { steps: { label: string; event?: string }[] }) {
  return (
    <ol className="flex flex-wrap items-center gap-1">
      {steps.map((s, i) => (
        <li key={i} className="flex items-center gap-1">
          {i > 0 && <ChevronRight size={12} className="text-neutral-400" aria-hidden="true" />}
          <span className="inline-flex items-center gap-1 rounded-lg bg-[#F0F4FC] px-2 py-1 text-[11px] font-semibold text-main-deep-blue">
            {s.event && <NodeIcon event={s.event} size={12} />}
            {s.label}
          </span>
        </li>
      ))}
    </ol>
  );
}

export interface ChipOption {
  value: string;
  label: string;
}

/** 칩 고르기 — `multiple`이면 여러 개, 아니면 하나만. */
export function ChipSelectView({
  label,
  options,
  multiple,
  value,
  disabled,
  onChange,
}: {
  label: string;
  options: ChipOption[];
  multiple?: boolean;
  value: string[];
  disabled?: boolean;
  onChange: (next: string[]) => void;
}) {
  const toggle = (v: string) => {
    const on = value.includes(v);
    onChange(multiple ? (on ? value.filter((x) => x !== v) : [...value, v]) : on ? [] : [v]);
  };
  return (
    <div role="group" aria-label={label} className="flex flex-wrap items-center gap-1.5">
      <span className={NAME}>{label}</span>
      {options.map((o) => {
        const on = value.includes(o.value);
        return (
          <button
            key={o.value}
            type="button"
            aria-pressed={on}
            disabled={disabled}
            onClick={() => toggle(o.value)}
            className={`inline-flex cursor-pointer items-center gap-1 rounded-full border px-2.5 py-1 font-semibold transition-colors disabled:cursor-default disabled:opacity-60 ${on ? "border-main-blue bg-main-light-blue text-main-blue" : "border-[#cde9f4] text-main-deep-blue hover:bg-main-blue/5"}`}
          >
            {on && <Check size={12} aria-hidden="true" />}
            {o.label}
          </button>
        );
      })}
    </div>
  );
}

/** 목록에서 하나 — 값이 선택지에 없으면 빈 칸을 보여 고른 척하지 않는다. */
export function SelectView({
  label,
  options,
  value,
  disabled,
  onChange,
}: {
  label: string;
  options: ChipOption[];
  value: string;
  disabled?: boolean;
  onChange: (next: string) => void;
}) {
  return (
    <label className="flex flex-col gap-1">
      <span className={NAME}>{label}</span>
      <select value={value} disabled={disabled} onChange={(e) => onChange(e.target.value)} className={BOX}>
        {!options.some((o) => o.value === value) && <option value="" />}
        {options.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
    </label>
  );
}

export function CheckboxView({ label, value, disabled, onChange }: { label: string; value: boolean; disabled?: boolean; onChange: (next: boolean) => void }) {
  return (
    <label className="flex items-center gap-1.5 text-neutral-700">
      <input type="checkbox" checked={value} disabled={disabled} onChange={(e) => onChange(e.target.checked)} className="size-3.5 accent-main-blue" />
      {label}
    </label>
  );
}

/** 글 입력 — 값 한도 200자(설계 1절 표). */
export function TextInputView({
  label,
  hint,
  value,
  disabled,
  onChange,
}: {
  label: string;
  hint?: string;
  value: string;
  disabled?: boolean;
  onChange: (next: string) => void;
}) {
  const hintId = useId();
  return (
    <div className="flex flex-col gap-1">
      <label className="flex flex-col gap-1">
        <span className={NAME}>{label}</span>
        <input
          value={value}
          maxLength={200}
          disabled={disabled}
          aria-describedby={hint ? hintId : undefined}
          onChange={(e) => onChange(e.target.value)}
          className={BOX}
        />
      </label>
      {hint && (
        <p id={hintId} className="text-[11px] text-neutral-500">
          {hint}
        </p>
      )}
    </div>
  );
}

/** 앱 전용 폴더 칸 — 라벨은 노드 칸 이름(LLM 글자를 쓰지 않는다), 경로는 화면에만 보인다. */
export function AppGateView({
  event,
  field,
  path,
  bound,
  disabled,
  canChange,
  onPick,
}: {
  event: string;
  field: string;
  path: string | null;
  bound: boolean;
  disabled: boolean;
  canChange: boolean;
  onPick: () => void;
}) {
  const label = tDyn(fieldKey(event, field)) ?? field;
  return (
    <div role="group" aria-label={label} className="flex flex-col gap-1.5 rounded-[10px] border-[1.5px] border-dashed border-slate-400 bg-slate-50 p-2">
      <p className="flex items-center gap-1.5 font-semibold text-slate-700">
        <FolderSearch size={13} aria-hidden="true" />
        {label}
        <span className={`${TAG} ml-auto bg-slate-100 text-slate-600`}>
          <ShieldCheck size={11} aria-hidden="true" />
          {t("chat.blueprint.appOnly")}
        </span>
      </p>
      {path ? (
        <p className="flex items-start gap-1.5 break-all rounded-lg bg-main-light-blue/50 px-2 py-1.5 font-semibold text-neutral-900">
          <Folder size={13} className="mt-px shrink-0 text-main-blue" aria-hidden="true" />
          {path}
        </p>
      ) : null}
      {path && canChange ? (
        <button type="button" disabled={disabled} onClick={onPick} className={`${CARD_BUTTON} bg-white`}>
          <RefreshCw size={13} aria-hidden="true" />
          {t("chat.blueprint.pickAgain")}
        </button>
      ) : null}
      {!path && bound ? (
        <p className="flex items-center gap-1.5 font-semibold text-neutral-700">
          <Check size={13} className="text-main-blue" aria-hidden="true" />
          {t("chat.blueprint.slotBound")}
        </p>
      ) : null}
      {!path && !bound ? (
        <button type="button" disabled={disabled} onClick={onPick} className={`${CARD_BUTTON} bg-white`}>
          <FolderOpen size={13} aria-hidden="true" />
          {t("chat.card.pickButton")}
        </button>
      ) : null}
    </div>
  );
}

export function SubmitButtonView({ disabled, needSlots, onSubmit }: { disabled: boolean; needSlots: boolean; onSubmit: () => void }) {
  return (
    <div className="flex flex-col gap-1">
      <button
        type="button"
        disabled={disabled}
        onClick={onSubmit}
        className="inline-flex cursor-pointer items-center justify-center rounded-xl bg-main-deep-blue px-3 py-2 text-xs font-semibold text-white transition-colors hover:bg-main-deep-blue/90 disabled:cursor-default disabled:opacity-40"
      >
        {t("chat.blueprint.submit")}
      </button>
      {disabled && needSlots && <p className="text-[11px] text-neutral-500">{t("chat.blueprint.needSlots")}</p>}
    </div>
  );
}
