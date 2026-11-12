import { Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { api, type StoredWorkflow, type ValidationError, type WfNode, type WorkflowEntry } from "../api";
import { Canvas, LocalBadge, nodeTitle } from "../editor/Canvas";
import { ConfigPanel } from "../editor/ConfigPanel";
import { Palette } from "../editor/Palette";
import { Toolbar } from "../editor/Toolbar";
import { useTestRun } from "../editor/useTestRun";
import { t } from "../i18n";
import { isLocal, newNode, nextPosition } from "../nodes/defs";
import { nodeMeta, toneStyle } from "../nodes/meta";
import { emptyLive, useRuns } from "../stores/runs";
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
  const [status, setStatus] = useState<{ saving?: boolean; error?: string }>({});
  const live = useRuns((s) => s.live[draft.id]) ?? emptyLive(draft.id);
  const testRun = useTestRun(draft.id);

  // 고칠 때마다 검증 결과를 노드 위에 보여 준다.
  useEffect(() => {
    let stale = false;
    api.validateWorkflow(draft.workflow).then(
      (e) => !stale && setErrors(e),
      (e) => !stale && setStatus({ error: String(e) }),
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
      setStatus({});
    } catch (e) {
      setStatus({ error: String(e) });
    }
  };

  const banner = [...general.map((e) => e.message), status.error, testRun.error].filter((m): m is string => !!m);
  const selectedMeta = selected && nodeMeta(selected.event, selected.type);

  return (
    <div className="flex h-full flex-col bg-white">
      <Toolbar
        name={draft.name}
        onName={(name) => setDraft({ ...draft, name })}
        dirty={dirty || !draft.id}
        enabled={draft.enabled}
        onEnabled={(enabled) => setDraft({ ...draft, enabled })}
        problems={errors.length}
        onBack={onBack}
        onOpenRuns={draft.id ? () => onOpenRuns(draft.id) : undefined}
        onSave={onSave}
        onRun={() => void testRun.run()}
        running={testRun.running}
        saving={!!status.saving}
      />
      {banner.length > 0 && (
        <ul className="flex flex-col gap-0.5 border-b border-danger-300 bg-danger-100 px-6 py-2 typo-caption1_semibold text-danger-700">
          {banner.map((m, i) => (
            <li key={i}>{m}</li>
          ))}
        </ul>
      )}
      <div className="flex min-h-0 flex-1">
        <aside className="w-60 shrink-0 overflow-y-auto border-r border-neutral-200 bg-white px-4 py-5">
          <Palette onAdd={add} />
        </aside>
        <div className="min-w-0 flex-1">
          <Canvas workflow={wf} errors={errors} statuses={live.nodes} selectedId={selectedId} onChange={setWorkflow} onSelect={setSelectedId} />
        </div>
        <aside className="flex w-80 shrink-0 flex-col overflow-y-auto border-l border-neutral-200 bg-white">
          {selected && selectedMeta ? (
            <>
              <div className="flex items-center gap-2.5 border-b border-neutral-200 px-5 py-4" style={toneStyle(selectedMeta)}>
                <span className="grid size-8 shrink-0 place-items-center rounded-[0.65rem] bg-(--node-color) text-white [box-shadow:0_4px_0_color-mix(in_srgb,var(--node-color)_78%,black)]">
                  {selectedMeta.icon}
                </span>
                <div className="min-w-0 flex-1">
                  <h2 className="truncate typo-body2_bold text-neutral-800">{selected.label || nodeTitle(selected.event)}</h2>
                  <p className="typo-caption1_medium text-(--node-color)">
                    {t("editor.step").replace("{n}", String(wf.nodes.indexOf(selected) + 1))} · {selectedMeta.label}
                  </p>
                </div>
                {isLocal(selected.event) && <LocalBadge />}
                <button
                  type="button"
                  className="grid size-8 shrink-0 place-items-center rounded-[10px] text-neutral-500 transition-colors hover:bg-danger-100 hover:text-danger-700"
                  onClick={() => remove(selected.id)}
                  aria-label={t("editor.deleteNode")}
                  title={t("editor.deleteNode")}
                >
                  <Trash2 size={16} aria-hidden="true" />
                </button>
              </div>
              <div className="p-5">
                <ConfigPanel key={selected.id} node={selected} workflow={wf} onChange={setNode} />
              </div>
            </>
          ) : (
            <p className="p-5 typo-body3_regular text-neutral-500">{t("editor.selectNode")}</p>
          )}
        </aside>
      </div>
    </div>
  );
}
