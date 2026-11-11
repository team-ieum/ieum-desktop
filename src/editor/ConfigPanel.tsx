import { FolderOpen, Plus, TriangleAlert, X } from "lucide-react";
import { useId } from "react";
import { api, type Workflow, type WfNode } from "../api";
import { t, tDyn } from "../i18n";
import { DEFS, fieldKey, FOLDER_FIELDS, newItem, outputKeys, upstream, type Field } from "../nodes/defs";
import { nodeTitle } from "./Canvas";

type Ref = { value: string; label: string };
type Obj = Record<string, unknown>;

const MAX_ITEMS = 10;
export const INPUT =
  "w-full rounded-brand-sm border border-neutral-300 bg-white px-3 py-2 typo-body3_regular text-neutral-800 outline-none focus:border-main-blue";
const SMALL_BUTTON =
  "inline-flex items-center gap-1 rounded-brand-sm border border-neutral-300 bg-white px-2.5 py-1.5 typo-caption1_semibold text-neutral-700 hover:bg-neutral-100 disabled:opacity-40";

interface FieldProps {
  event: string;
  field: Field;
  path: string;
  value: unknown;
  onChange: (v: unknown) => void;
  refs: Ref[];
  pickFolder: () => Promise<string | null>;
}

function FieldInput({ event, field, path, value, onChange, refs, pickFolder }: FieldProps) {
  const id = useId();
  const label = tDyn(fieldKey(event, path)) ?? field.title ?? field.key;
  const desc = tDyn(`fieldDesc.${event}.${path}`);
  const str = typeof value === "string" ? value : "";

  let input;
  if (FOLDER_FIELDS.has(field.key)) {
    // 사람이 고른 폴더만 — 직접 입력·참조 금지.
    input = (
      <div className="flex items-center gap-2">
        <span className={`min-w-0 flex-1 truncate typo-body3_regular ${str ? "text-neutral-800" : "text-neutral-400"}`} title={str}>
          {str || t("editor.noFolder")}
        </span>
        <button
          type="button"
          className={SMALL_BUTTON}
          onClick={async () => {
            const picked = await pickFolder();
            if (picked) onChange(picked);
          }}
        >
          <FolderOpen size={14} aria-hidden="true" />
          {t("editor.pickFolder")}
        </button>
      </div>
    );
  } else if (field.list && field.children) {
    const items = Array.isArray(value) ? (value as Obj[]) : [];
    const set = (i: number, item: Obj) => onChange(items.map((x, j) => (j === i ? item : x)));
    input = (
      <div className="flex flex-col gap-2">
        {items.map((item, i) => (
          <div key={i} className="relative flex flex-col gap-3 rounded-brand-sm border border-neutral-200 bg-neutral-50 p-3 pr-9">
            <button
              type="button"
              className="absolute right-2 top-2 rounded-full p-1 text-neutral-500 hover:bg-neutral-200"
              aria-label={t("editor.removeItem")}
              title={t("editor.removeItem")}
              onClick={() => onChange(items.filter((_, j) => j !== i))}
            >
              <X size={14} aria-hidden="true" />
            </button>
            {field.children!
              // 갈래 id는 시스템이 만든다(계약) — 보이지 않게.
              .filter((c) => !(field.key === "branches" && c.key === "key"))
              .map((c) => (
                <FieldInput
                  key={c.key}
                  event={event}
                  field={c}
                  path={`${path}.${c.key}`}
                  value={item?.[c.key]}
                  onChange={(v) => set(i, { ...item, [c.key]: v })}
                  refs={refs}
                  pickFolder={pickFolder}
                />
              ))}
          </div>
        ))}
        <button
          type="button"
          className={`${SMALL_BUTTON} self-start`}
          disabled={items.length >= MAX_ITEMS}
          onClick={() => onChange([...items, newItem(field.children!, field.key)])}
        >
          <Plus size={14} aria-hidden="true" />
          {t("editor.addItem")}
        </button>
      </div>
    );
  } else if (field.choices) {
    input = (
      <select id={id} className={INPUT} value={str} onChange={(e) => onChange(e.target.value)}>
        {!str && <option value="" disabled />}
        {field.choices.map((c) => (
          <option key={c.id} value={c.id}>
            {tDyn(`choice.${field.key}.${c.id}`) ?? c.name}
          </option>
        ))}
      </select>
    );
  } else if (field.type === "boolean") {
    input = (
      <div className="flex flex-col gap-1.5">
        <input id={id} type="checkbox" className="size-4 accent-main-blue" checked={value === true} onChange={(e) => onChange(e.target.checked)} />
        {field.key === "sendContent" && value === true && (
          <p className="flex items-start gap-1.5 rounded-brand-sm bg-danger-100 px-2.5 py-2 typo-caption1_semibold text-danger-700" role="note">
            <TriangleAlert size={14} className="mt-0.5 shrink-0" aria-hidden="true" />
            {t("editor.sendContentWarning")}
          </p>
        )}
      </div>
    );
  } else {
    const Tag = field.type === "text" ? "textarea" : "input";
    input = (
      <div className="flex flex-col gap-1.5">
        <Tag id={id} className={`${INPUT} ${Tag === "textarea" ? "min-h-20 resize-y" : ""}`} value={str} onChange={(e) => onChange(e.target.value)} />
        {field.ref !== false && refs.length > 0 && (
          <select
            className={`${INPUT} typo-caption1_regular text-neutral-600`}
            aria-label={t("editor.insertRef")}
            value=""
            onChange={(e) => e.target.value && onChange(str + e.target.value)}
          >
            <option value="">{t("editor.insertRef")}</option>
            {refs.map((r) => (
              <option key={r.value} value={r.value}>
                {r.label}
              </option>
            ))}
          </select>
        )}
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-1.5" data-testid={`field-${field.key}`}>
      <label htmlFor={id} className="typo-label_semibold text-neutral-700">
        {label}
        {field.required && <span className="text-danger-600"> *</span>}
      </label>
      {desc && <p className="typo-caption1_regular text-neutral-500">{desc}</p>}
      {input}
    </div>
  );
}

const LEGACY_BRANCH_FIELDS = new Set(["operator", "left", "right"]);

export interface ConfigPanelProps {
  node: WfNode;
  workflow: Workflow;
  onChange: (node: WfNode) => void;
  pickFolder?: () => Promise<string | null>;
}

export function ConfigPanel({ node, workflow, onChange, pickFolder = () => api.pickPath(true) }: ConfigPanelProps) {
  const labelId = useId();
  const def = DEFS[node.event];
  const refs: Ref[] = upstream(workflow, node.id).flatMap((n) =>
    outputKeys(n).map((k) => ({ value: `{{nodes.${n.id}.output.${k}}}`, label: `${n.label || nodeTitle(n.event)} · ${k}` })),
  );
  const hasBranches = Array.isArray(node.config.branches) && node.config.branches.length > 0;
  const fields = (def?.inputFields ?? []).filter((f) => !(node.event === "branch.if" && hasBranches && LEGACY_BRANCH_FIELDS.has(f.key)));

  return (
    <div className="flex flex-col gap-5">
      <div className="flex flex-col gap-1.5">
        <label htmlFor={labelId} className="typo-label_semibold text-neutral-700">
          {t("editor.label")}
        </label>
        <input id={labelId} className={INPUT} value={node.label ?? ""} onChange={(e) => onChange({ ...node, label: e.target.value })} />
      </div>
      {fields.map((f) => (
        <FieldInput
          key={f.key}
          event={node.event}
          field={f}
          path={f.key}
          value={node.config[f.key]}
          onChange={(v) => onChange({ ...node, config: { ...node.config, [f.key]: v } })}
          refs={refs}
          pickFolder={pickFolder}
        />
      ))}
    </div>
  );
}
