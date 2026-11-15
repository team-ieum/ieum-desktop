import { EdgeToolbar, getBezierPath, useReactFlow, type Edge, type EdgeProps } from "@xyflow/react";
import { Trash2 } from "lucide-react";
import { t } from "../i18n";

export type EdgeData = { flowing?: boolean; readOnly?: boolean };

/** 웹 AnimatedEdge — 대상 노드가 실행 중이면 점선이 흐른다(CSS `ieum-flow`). */
export function AnimatedEdge({ id, sourceX, sourceY, targetX, targetY, sourcePosition, targetPosition, markerEnd, data, selected }: EdgeProps<Edge<EdgeData>>) {
  const { deleteElements } = useReactFlow();
  const [path, x, y] = getBezierPath({ sourceX, sourceY, sourcePosition, targetX, targetY, targetPosition });
  return (
    <>
      <path id={id} d={path} fill="none" className="react-flow__edge-path" style={{ stroke: "#a8b0ba", strokeWidth: selected ? 2.5 : 1.5 }} markerEnd={markerEnd} />
      {data?.flowing && (
        <path d={path} fill="none" className="ieum-flow" style={{ stroke: "#007ba7", strokeWidth: 2.5, strokeDasharray: "6 8", strokeLinecap: "round" }} />
      )}
      <EdgeToolbar edgeId={id} x={x} y={y} isVisible={!!selected && !data?.readOnly}>
        <button
          type="button"
          aria-label={t("editor.deleteEdge")}
          title={t("editor.deleteEdge")}
          onClick={() => void deleteElements({ edges: [{ id }] })}
          className="nodrag nopan grid size-9 cursor-pointer place-items-center rounded-full border border-red-200 bg-white text-red-500 shadow-md transition-colors hover:bg-red-50"
        >
          <Trash2 size={16} aria-hidden="true" />
        </button>
      </EdgeToolbar>
    </>
  );
}
