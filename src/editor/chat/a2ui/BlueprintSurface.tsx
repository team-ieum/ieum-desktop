import { A2uiSurface, createComponentImplementation, type ReactCatalogComponent } from "@a2ui/react";
import { Catalog, MessageProcessor } from "@a2ui/web_core";
import { CommonSchemas } from "@a2ui/web_core/v1_0";
import { useEffect, useMemo } from "react";
import { z } from "zod";
import type { Blueprint } from "../../../api";
import { BlueprintView, ChipSelectView } from "./views";

const CATALOG_ID = "ieum.chat.blueprint/v1";
const SURFACE_ID = "blueprint";

export interface BlueprintSurfaceProps {
  blueprint: Blueprint;
  locked: boolean;
  busy: boolean;
  changed?: string[];
  onSubmit(data: Record<string, unknown>): void;
  onRetry(): void;
  onPickSlot(slot: string): void;
  onData(data: Record<string, unknown>): void;
}

// 구현 객체는 모듈에서 한 번만 — 다시 만들면 태그 이름이 -2, -3으로 늘어난다.
const BlueprintImpl = createComponentImplementation(
  { name: "Blueprint", schema: z.object({ title: z.string(), children: CommonSchemas.ChildList }) },
  ({ props, buildChild }) => (
    <BlueprintView title={props.title}>
      {(props.children ?? []).map((c) => (typeof c === "string" ? buildChild(c) : buildChild(c.id, c.basePath)))}
    </BlueprintView>
  ),
);

const ChipSelectImpl = createComponentImplementation(
  {
    name: "ChipSelect",
    schema: z.object({
      label: z.string(),
      multiple: z.boolean().optional(),
      options: z.array(z.object({ value: z.string(), label: z.string() })),
      value: CommonSchemas.DynamicStringList,
    }),
  },
  ({ props }) => (
    <ChipSelectView
      label={props.label}
      options={props.options}
      multiple={props.multiple}
      value={props.value ?? []}
      onChange={(v) => props.setValue(v)}
    />
  ),
);

const CATALOG = new Catalog<ReactCatalogComponent>(CATALOG_ID, "1.0", [BlueprintImpl, ChipSelectImpl]);

/** 설계서 카드 — A2UI v1.0 렌더러는 이 파일에만 들인다. 버전·상태가 바뀌면 표면을 새로 만든다. */
export default function BlueprintSurface({ blueprint, onData }: BlueprintSurfaceProps) {
  const { version, status, components, dataModel } = blueprint;
  const processor = useMemo(() => {
    void version;
    void status;
    const p = new MessageProcessor<ReactCatalogComponent>([CATALOG]);
    p.processMessagesWithErrors([
      { version: "1.0", createSurface: { surfaceId: SURFACE_ID, catalogId: CATALOG_ID, components, dataModel } },
    ]);
    return p;
    // 같은 버전·상태에서는 부모가 새 객체를 줘도 다시 만들지 않는다.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [version, status]);
  useEffect(() => () => processor.dispose(), [processor]);
  useEffect(() => {
    const dm = processor.getSurface(SURFACE_ID)?.dataModel;
    if (!dm) return;
    const sub = dm.subscribe("/", () => onData(dm.get("/") as Record<string, unknown>));
    return () => sub.unsubscribe();
  }, [processor, onData]);
  const surface = processor.getSurface(SURFACE_ID);
  return surface ? <A2uiSurface surface={surface} /> : <></>;
}
