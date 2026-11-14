import { FolderOpen, Plus, TriangleAlert, X } from "lucide-react";
import { useEffect, useId } from "react";
import { api, type Workflow, type WfNode } from "../api";
import { ProviderLogo, type LogoId } from "../assets/providers";
import { t, tDyn } from "../i18n";
import { credLabel } from "../screens/settings/AiPane";
import { choicesFor, useCredentials } from "../stores/credentials";
import { DEFS, fieldKey, FOLDER_FIELDS, newItem, outputKeys, upstream, type Field } from "../nodes/defs";
import { Dropdown } from "../usage/parts";
import { nodeTitle } from "./Canvas";
import { modelItems, useModels } from "./models";

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
  /** AI 노드의 지금 공급원 — 키·계정 칸이 본다. */
  provider?: unknown;
}

const PROVIDER_LOGO: Record<string, LogoId> = { "cli:claude": "claude", "cli:codex": "openai", "cli:agy": "antigravity", "api:anthropic": "anthropic", "api:openai": "openai", "api:gemini": "gemini" };

/** AI 노드 "키·계정" — 그 공급원의 목록 + 맨 위 "기본(이름표)". 비우면 칸을 저장하지 않는다(`undefined`). */
function CredentialSelect({ id, provider, value, onChange }: { id: string; provider: unknown; value: string; onChange: (v: unknown) => void }) {
  const view = useCredentials((s) => s.view);
  useEffect(() => {
    useCredentials.getState().load().catch(() => {});
  }, []);
  const list = view ? choicesFor(view, provider) : [];
  const def = list.find((m) => m.isDefault);
  // 목록을 읽었는데 저장된 id가 없으면 경고(다른 키로 대신하지 않는다).
  const missing = !!view && !!value && !list.some((m) => m.id === value);
  return (
    <div className="flex flex-col gap-1.5">
      <select id={id} className={`${INPUT} ${missing ? "border-danger-600" : ""}`} value={value} onChange={(e) => onChange(e.target.value || undefined)}>
        <option value="">{def ? t("editor.credentialDefault").replace("{label}", credLabel(def)) : t("settings.defaultBadge")}</option>
        {list.map((m) => (
          <option key={m.id} value={m.id}>
            {credLabel(m)}
          </option>
        ))}
        {value && !list.some((m) => m.id === value) && <option value={value}>{view ? t("editor.credentialMissing") : value}</option>}
      </select>
      {missing && (
        <p role="alert" className="flex items-center gap-1.5 typo-caption1_semibold text-danger-700">
          <TriangleAlert size={14} aria-hidden="true" />
          {t("editor.credentialMissing")}
        </p>
      )}
    </div>
  );
}

/** AI 노드 "모델" — 고른 공급원의 모델 목록. "기본 모델"이면 칸을 저장하지 않는다(`undefined`). 목록에 없는 저장값은 그대로 둔다. */
function ModelSelect({ label, provider, value, onChange }: { label: string; provider: unknown; value: string; onChange: (v: unknown) => void }) {
  const p = typeof provider === "string" ? provider : undefined;
  const { models, failed } = useModels(p);
  return (
    <div className="flex flex-col gap-1.5">
      <Dropdown
        label={label}
        items={modelItems(p, models, value)}
        sel={value}
        onSel={(id) => onChange(id || undefined)}
        left
        root="relative w-full"
        trigger={`${INPUT} flex items-center justify-between gap-2 text-left hover:bg-neutral-50`}
      />
      {failed && (
        <p role="alert" className="typo-caption1_regular text-danger-700">
          {t("model.listFailed")}
        </p>
      )}
    </div>
  );
}

function FieldInput({ event, field, path, value, onChange, refs, pickFolder, provider }: FieldProps) {
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
                  // 목록이 ref:false면 자식 칸도 참조 금지(엔진 검증과 같게)
                  refs={field.ref === false ? [] : refs}
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
  } else if (event === "local.ai" && field.key === "credential") {
    input = <CredentialSelect id={id} provider={provider} value={str} onChange={onChange} />;
  } else if (event === "local.ai" && field.key === "model") {
    input = <ModelSelect label={label} provider={provider} value={str} onChange={onChange} />;
  } else if (field.choices) {
    const logo = event === "local.ai" && field.key === "provider" ? PROVIDER_LOGO[str] : undefined;
    input = (
      <div className="flex items-center gap-2">
        {logo && <ProviderLogo id={logo} size={28} />}
        <select id={id} className={INPUT} value={str} onChange={(e) => onChange(e.target.value)}>
          {!str && <option value="" disabled />}
          {field.choices.map((c) => (
            <option key={c.id} value={c.id}>
              {tDyn(`choice.${field.key}.${c.id}`) ?? c.name}
            </option>
          ))}
        </select>
      </div>
    );
  } else if (field.type === "boolean") {
    // Antigravity는 파일 내용을 못 받는다(러너도 거절한다) — 칸을 잠근다.
    const locked = field.key === "sendContent" && provider === "cli:agy";
    input = (
      <div className="flex flex-col gap-1.5">
        <input
          id={id}
          type="checkbox"
          className="size-4 accent-main-blue"
          checked={value === true && !locked}
          disabled={locked}
          onChange={(e) => onChange(e.target.checked)}
        />
        {locked && (
          <p className="typo-caption1_regular text-neutral-500" role="note">
            {t("editor.sendContentAgy")}
          </p>
        )}
        {field.key === "sendContent" && value === true && !locked && (
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
          onChange={(v) => {
            // undefined = 칸을 뺀다. 공급원을 바꾸면 고른 키·계정·모델(다른 공급원의 것)도 뺀다.
            const { [f.key]: _old, ...rest } = node.config;
            if (node.event === "local.ai" && f.key === "provider") {
              delete rest.credential;
              delete rest.model;
              if (v === "cli:agy") delete rest.sendContent;
            }
            onChange({ ...node, config: v === undefined ? rest : { ...rest, [f.key]: v } });
          }}
          refs={refs}
          pickFolder={pickFolder}
          provider={node.config.provider}
        />
      ))}
    </div>
  );
}
