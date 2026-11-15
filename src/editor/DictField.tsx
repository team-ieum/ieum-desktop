import { Plus, TriangleAlert, X } from "lucide-react";
import { useState, type ReactNode } from "react";
import { t } from "../i18n";
import { INPUT, SMALL_BUTTON } from "./ConfigPanel";

/** 화면 글(draft)은 사람이 친 그대로 두고 저장값만 따로 낸다. 바깥 값이 우리가 낸 것과 다르게 바뀌면(되돌리기 등) 다시 읽는다. */
export function useDraft<T>(value: unknown, read: (v: unknown) => T, onChange: (v: unknown) => void) {
  const [draft, setDraft] = useState(() => read(value));
  const [seen, setSeen] = useState(value);
  if (value !== seen) {
    setSeen(value);
    setDraft(read(value));
  }
  const edit = (next: T, save?: { value: unknown }) => {
    setDraft(next);
    if (save) {
      setSeen(save.value);
      onChange(save.value);
    }
  };
  return [draft, edit] as const;
}

type Row = { id: number; key: string; value: unknown };
let nextId = 0;

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);
const toRows = (v: unknown): Row[] => (isObj(v) ? Object.entries(v).map(([key, value]) => ({ id: nextId++, key, value })) : []);
// 숫자·객체 값은 고치기 전까지 그대로 저장한다.
const shown = (v: unknown) => (typeof v === "string" ? v : JSON.stringify(v) ?? "");
const blank = (k: string) => k.trim() === "";

/** 같은 키가 또 나오면 그 줄은 저장하지 않는다(먼저 나온 줄이 이긴다). */
function duplicates(rows: Row[]): Set<number> {
  const seen = new Set<string>();
  const dup = new Set<number>();
  for (const r of rows) {
    if (blank(r.key)) continue;
    if (seen.has(r.key)) dup.add(r.id);
    else seen.add(r.key);
  }
  return dup;
}

const toObject = (rows: Row[], dup: Set<number>) =>
  Object.fromEntries(rows.filter((r) => !blank(r.key) && !dup.has(r.id)).map((r) => [r.key, r.value]));

const fill = (s: string, label: string, n: number) => s.replace("{label}", label).replace("{n}", String(n));

export function DictField({
  label,
  value,
  onChange,
  refPicker,
}: {
  label: string;
  value: unknown;
  onChange: (next: unknown) => void;
  /** 값 칸 아래 "앞 단계 값 넣기" — 고른 참조식을 받아 붙인다. */
  refPicker?: (pick: (ref: string) => void) => ReactNode;
}) {
  const [rows, edit] = useDraft(value, toRows, onChange);
  const [focusId, setFocusId] = useState<number | null>(null);

  // 참조식 글(빌더가 글 그대로 둔 값)은 지우지 않고 글 칸으로.
  if (typeof value === "string" && value !== "") {
    return (
      <div className="flex flex-col gap-1.5">
        <input aria-label={label} className={INPUT} value={value} onChange={(e) => onChange(e.target.value)} />
        {refPicker?.((ref) => onChange(value + ref))}
      </div>
    );
  }

  const save = (next: Row[]) => edit(next, { value: toObject(next, duplicates(next)) });
  const set = (id: number, patch: Partial<Row>) => save(rows.map((r) => (r.id === id ? { ...r, ...patch } : r)));
  const dup = duplicates(rows);

  return (
    <div className="flex flex-col gap-2">
      {rows.map((r, i) => {
        const n = i + 1;
        const dupId = `dict-dup-${r.id}`;
        return (
          <div key={r.id} className="flex flex-col gap-1.5">
            <div className="flex items-start gap-2">
              <input
                aria-label={fill(t("editor.dictKey"), label, n)}
                aria-invalid={dup.has(r.id) || undefined}
                aria-describedby={dup.has(r.id) ? dupId : undefined}
                placeholder={t("editor.dictKeyHint")}
                autoFocus={r.id === focusId}
                className={`${INPUT} w-1/3 ${dup.has(r.id) ? "border-danger-600" : ""}`}
                value={r.key}
                onChange={(e) => set(r.id, { key: e.target.value })}
              />
              <div className="flex min-w-0 flex-1 flex-col gap-1.5">
                <input
                  aria-label={fill(t("editor.dictValue"), label, n)}
                  placeholder={t("editor.dictValueHint")}
                  className={INPUT}
                  value={shown(r.value)}
                  onChange={(e) => set(r.id, { value: e.target.value })}
                />
                {refPicker?.((ref) => set(r.id, { value: shown(r.value) + ref }))}
              </div>
              <button
                type="button"
                className="mt-1.5 rounded-full p-1 text-neutral-500 hover:bg-neutral-200"
                aria-label={fill(t("editor.dictRemove"), label, n)}
                title={t("editor.removeItem")}
                onClick={() => save(rows.filter((x) => x.id !== r.id))}
              >
                <X size={14} aria-hidden="true" />
              </button>
            </div>
            {dup.has(r.id) && (
              <p id={dupId} className="flex items-center gap-1.5 typo-caption1_semibold text-danger-700">
                <TriangleAlert size={14} aria-hidden="true" />
                {t("editor.dictDuplicate")}
              </p>
            )}
          </div>
        );
      })}
      <button
        type="button"
        className={`${SMALL_BUTTON} self-start`}
        onClick={() => {
          const id = nextId++;
          setFocusId(id);
          // 빈 키 줄은 저장값에 없으니 화면 줄만 늘린다.
          edit([...rows, { id, key: "", value: "" }]);
        }}
      >
        <Plus size={14} aria-hidden="true" />
        {t("editor.dictAdd")}
      </button>
    </div>
  );
}
