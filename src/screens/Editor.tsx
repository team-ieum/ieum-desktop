import { ArrowLeft, CircleAlert, Play, Save, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { api, type StoredWorkflow, type ValidationError, type WfNode, type WorkflowEntry } from "../api";
import { Canvas, nodeTitle } from "../editor/Canvas";
import { ConfigPanel } from "../editor/ConfigPanel";
import { Palette } from "../editor/Palette";
import { t } from "../i18n";
import { newNode, nextPosition } from "../nodes/defs";
import { useWorkflows } from "../stores/workflows";

export const BUTTON =
  "inline-flex items-center gap-1.5 rounded-brand-sm border border-neutral-300 bg-white px-3 py-2 typo-button1_semibold text-neutral-700 hover:bg-neutral-100 disabled:opacity-40";
export const PRIMARY =
  "inline-flex items-center gap-1.5 rounded-brand-sm bg-main-blue px-3 py-2 typo-button1_semibold text-white hover:bg-main-deep-blue disabled:opacity-40";

const strip = ({ watch: _w, watchMessage: _m, ...wf }: WorkflowEntry): StoredWorkflow => wf;

export function Editor({
  id,
  onBack,
  onOpenRuns,
  onDirty,
}: {
  id: string | null;
  onBack: () => void;
  onOpenRuns: (id: string) => void;
  /** 저장본과 다른지 — 화면을 떠날 때 확인 줄을 띄우는 데 쓴다. */
  onDirty?: (dirty: boolean) => void;
}) {
  const entry = useWorkflows((s) => s.list.find((w) => w.id === id));
  const save = useWorkflows((s) => s.save);
  const [draft, setDraft] = useState<StoredWorkflow>(() =>
    entry ? strip(entry) : { id: "", name: t("editor.untitled"), enabled: true, workflow: { nodes: [], edges: [] }, updatedAt: "" },
  );
  const [base, setBase] = useState(draft);
  const dirty = JSON.stringify(draft) !== JSON.stringify(base);
  useEffect(() => {
    onDirty?.(dirty);
    return () => onDirty?.(false);
  }, [dirty, onDirty]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [errors, setErrors] = useState<ValidationError[]>([]);
  const [status, setStatus] = useState<{ saving?: boolean; message?: string; error?: boolean }>({});

  // 고칠 때마다 검증 결과를 노드 위에 보여 준다.
  useEffect(() => {
    let stale = false;
    api.validateWorkflow(draft.workflow).then(
      (e) => !stale && setErrors(e),
      (e) => !stale && setStatus({ message: String(e), error: true }),
    );
    return () => {
      stale = true;
    };
  }, [draft.workflow]);

  const wf = draft.workflow;
  const selected = wf.nodes.find((n) => n.id === selectedId);
  const known = new Set(wf.nodes.map((n) => n.id));
  const general = errors.filter((e) => !e.nodeId || !known.has(e.nodeId));
  const setWorkflow = (workflow: StoredWorkflow["workflow"]) => setDraft((d) => ({ ...d, workflow }));
  const setNode = (node: WfNode) => setWorkflow({ ...wf, nodes: wf.nodes.map((n) => (n.id === node.id ? node : n)) });

  const add = (event: string) => {
    const node = newNode(event, nextPosition(wf.nodes), nodeTitle(event));
    setWorkflow({ ...wf, nodes: [...wf.nodes, node] });
    setSelectedId(node.id);
  };

  const remove = (nodeId: string) => {
    setWorkflow({
      ...wf,
      nodes: wf.nodes.filter((n) => n.id !== nodeId),
      edges: wf.edges.filter((e) => e.source !== nodeId && e.target !== nodeId),
    });
    setSelectedId(null);
  };

  const onSave = async () => {
    setStatus({ saving: true });
    try {
      const saved = await save(draft);
      setDraft(saved);
      setBase(saved);
      setStatus({ message: t("common.saved") });
    } catch (e) {
      setStatus({ message: String(e), error: true });
    }
  };

  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center gap-3 border-b border-neutral-200 bg-white px-4 py-3">
        <button type="button" className={BUTTON} onClick={onBack} aria-label={t("common.back")}>
          <ArrowLeft size={16} aria-hidden="true" />
        </button>
        <input
          className="min-w-0 flex-1 rounded-brand-sm border border-transparent px-2 py-1.5 typo-title3_bold text-neutral-800 outline-none hover:border-neutral-300 focus:border-main-blue"
          aria-label={t("editor.name")}
          value={draft.name}
          onChange={(e) => setDraft({ ...draft, name: e.target.value })}
        />
        {errors.length > 0 && (
          <span className="inline-flex items-center gap-1.5 rounded-full border border-danger-300 bg-danger-100 px-2.5 py-1 typo-caption1_semibold text-danger-700">
            <CircleAlert size={12} aria-hidden="true" />
            {t("editor.problems")} {errors.length}
          </span>
        )}
        {status.message && (
          <span className={`typo-caption1_semibold ${status.error ? "text-danger-700" : "text-main-deep-blue"}`} role="status">
            {status.message}
          </span>
        )}
        <label className="flex items-center gap-1.5 typo-body3_semibold text-neutral-700">
          <input
            type="checkbox"
            className="size-4 accent-main-blue"
            checked={draft.enabled}
            onChange={(e) => setDraft({ ...draft, enabled: e.target.checked })}
          />
          {t("editor.enabled")}
        </label>
        <button type="button" className={BUTTON} disabled={!draft.id} title={draft.id ? undefined : t("run.saveFirst")} onClick={() => onOpenRuns(draft.id)}>
          <Play size={16} aria-hidden="true" />
          {t("list.runView")}
        </button>
        <button type="button" className={PRIMARY} disabled={status.saving} onClick={onSave}>
          <Save size={16} aria-hidden="true" />
          {status.saving ? t("common.saving") : t("common.save")}
        </button>
      </header>
      {general.length > 0 && (
        <ul className="flex flex-col gap-0.5 border-b border-danger-300 bg-danger-100 px-4 py-2 typo-caption1_semibold text-danger-700">
          {general.map((e, i) => (
            <li key={i}>{e.message}</li>
          ))}
        </ul>
      )}
      <div className="flex min-h-0 flex-1">
        <aside className="w-64 shrink-0 overflow-y-auto border-r border-neutral-200 bg-neutral-50 p-4">
          <Palette onAdd={add} />
        </aside>
        <div className="min-w-0 flex-1">
          <Canvas workflow={wf} errors={errors} selectedId={selectedId} onChange={setWorkflow} onSelect={setSelectedId} />
        </div>
        <aside className="w-80 shrink-0 overflow-y-auto border-l border-neutral-200 bg-white p-4">
          {selected ? (
            <div className="flex flex-col gap-4">
              <div className="flex items-center gap-2">
                <h2 className="min-w-0 flex-1 truncate typo-body1_bold text-neutral-800">{nodeTitle(selected.event)}</h2>
                <button type="button" className={BUTTON} onClick={() => remove(selected.id)} aria-label={t("editor.deleteNode")} title={t("editor.deleteNode")}>
                  <Trash2 size={14} aria-hidden="true" />
                </button>
              </div>
              <ConfigPanel key={selected.id} node={selected} workflow={wf} onChange={setNode} />
            </div>
          ) : (
            <p className="typo-body3_regular text-neutral-500">{t("editor.selectNode")}</p>
          )}
        </aside>
      </div>
    </div>
  );
}
