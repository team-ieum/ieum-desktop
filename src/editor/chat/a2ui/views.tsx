import type { ReactNode } from "react";
import { CARD } from "../ChatCards";

/** 설계서 카드 바깥틀 — 제목과 자식 컴포넌트를 담는다. 글자는 LLM이 보낸 그대로. */
export function BlueprintView({ title, children }: { title: string; children?: ReactNode }) {
  return (
    <section role="group" aria-label={title} className={`${CARD} max-w-none flex flex-col gap-2.5`}>
      <h3 className="text-xs font-semibold text-neutral-800">{title}</h3>
      {children}
    </section>
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
  onChange,
}: {
  label: string;
  options: ChipOption[];
  multiple?: boolean;
  value: string[];
  onChange: (next: string[]) => void;
}) {
  const toggle = (v: string) => {
    const on = value.includes(v);
    onChange(multiple ? (on ? value.filter((x) => x !== v) : [...value, v]) : on ? [] : [v]);
  };
  return (
    <div role="group" aria-label={label} className="flex flex-wrap items-center gap-1.5">
      <span className="text-neutral-500">{label}</span>
      {options.map((o) => {
        const on = value.includes(o.value);
        return (
          <button
            key={o.value}
            type="button"
            aria-pressed={on}
            onClick={() => toggle(o.value)}
            className={`cursor-pointer rounded-full border px-2.5 py-1 transition-colors ${on ? "border-main-blue bg-main-blue/10 text-main-blue" : "border-[#cde9f4] text-neutral-600 hover:bg-main-blue/5"}`}
          >
            {o.label}
          </button>
        );
      })}
    </div>
  );
}
