import { A2uiSurface, createComponentImplementation, type ReactCatalogComponent } from "@a2ui/react";
import { Catalog, MessageProcessor, type SurfaceModel } from "@a2ui/web_core";
import { CommonSchemas } from "@a2ui/web_core/v1_0";
import { useEffect, useRef, useState } from "react";
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
  const { version, status } = blueprint;
  const [surface, setSurface] = useState<SurfaceModel<ReactCatalogComponent>>();
  const latest = useRef(blueprint);
  latest.current = blueprint;
  const onDataRef = useRef(onData);
  onDataRef.current = onData;
  // 만들기·구독·dispose를 한 effect 수명으로 묶는다 — StrictMode의 cleanup→재실행에도 짝이 맞는다.
  useEffect(() => {
    const { components, dataModel } = latest.current;
    const p = new MessageProcessor<ReactCatalogComponent>([CATALOG]);
    p.processMessagesWithErrors([
      { version: "1.0", createSurface: { surfaceId: SURFACE_ID, catalogId: CATALOG_ID, components, dataModel } },
    ]);
    const s = p.getSurface(SURFACE_ID);
    // DataModel이 제자리에서 바뀌므로 복사본을 넘긴다.
    const sub = s?.dataModel.subscribe("/", () => onDataRef.current(structuredClone(s.dataModel.get("/")) as Record<string, unknown>));
    setSurface(s);
    return () => {
      sub?.unsubscribe();
      p.dispose();
      setSurface(undefined);
    };
  }, [version, status]);
  return surface ? <A2uiSurface surface={surface} /> : <></>;
}
