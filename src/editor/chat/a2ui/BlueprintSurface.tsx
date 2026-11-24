import { A2uiSurface, createComponentImplementation, type ReactCatalogComponent } from "@a2ui/react";
import { Catalog, MessageProcessor, type SurfaceModel } from "@a2ui/web_core";
import { CommonSchemas } from "@a2ui/web_core/v1_0";
import { RotateCw } from "lucide-react";
import { createContext, useContext, useEffect, useRef, useState } from "react";
import { z } from "zod";
import type { Blueprint } from "../../../api";
import { t } from "../../../i18n";
import { CARD_BUTTON } from "../ChatCards";
import {
  AppGateView,
  BlueprintView,
  Changed,
  CheckboxView,
  ChipSelectView,
  FlowPreviewView,
  SelectView,
  SubmitButtonView,
  TextInputView,
} from "./views";

const CATALOG_ID = "ieum.chat.blueprint/v1";
const SURFACE_ID = "blueprint";

export interface BlueprintSurfaceProps {
  blueprint: Blueprint;
  locked: boolean;
  busy: boolean;
  changed?: string[];
  onSubmit(data: Record<string, unknown>): void;
  onRetry?: () => void;
  onPickSlot(slot: string): void;
  onData(data: Record<string, unknown>): void;
}

/** 표면 안 컴포넌트가 읽는 앱 상태 — 데이터 모델이 아니라 래퍼 props에서 온다(슬롯 경로는 LLM에게 가지 않는다). */
interface Live {
  status: Blueprint["status"];
  slots: Blueprint["slots"];
  locked: boolean;
  busy: boolean;
  changed: string[];
  onPickSlot(slot: string): void;
  submit(): void;
}
const LiveContext = createContext({} as Live);
const useLive = () => useContext(LiveContext);

const Options = z.array(z.object({ value: z.string(), label: z.string() }));

// 구현 객체는 모듈에서 한 번만 — 다시 만들면 태그 이름이 -2, -3으로 늘어난다.
// prop은 Rust 검증 표(`builder/blueprint.rs` COMPONENTS)와 같다. 선택 prop은 multiple·hint·step.event뿐.
const BlueprintImpl = createComponentImplementation(
  { name: "Blueprint", schema: z.object({ title: z.string(), children: CommonSchemas.ChildList }) },
  ({ props, buildChild }) => {
    const { status, changed } = useLive();
    return (
      <BlueprintView title={props.title} status={status}>
        {(props.children ?? []).map((c) => {
          const [id, base] = typeof c === "string" ? [c, undefined] : [c.id, c.basePath];
          return (
            <Changed key={id} on={changed.includes(id)}>
              {buildChild(id, base)}
            </Changed>
          );
        })}
      </BlueprintView>
    );
  },
);

const FlowPreviewImpl = createComponentImplementation(
  { name: "FlowPreview", schema: z.object({ steps: z.array(z.object({ label: z.string(), event: z.string().optional() })) }) },
  ({ props }) => <FlowPreviewView steps={props.steps} />,
);

const ChipSelectImpl = createComponentImplementation(
  {
    name: "ChipSelect",
    schema: z.object({ label: z.string(), multiple: z.boolean().optional(), options: Options, value: CommonSchemas.DynamicStringList }),
  },
  ({ props }) => (
    <ChipSelectView
      label={props.label}
      options={props.options}
      multiple={props.multiple}
      value={props.value ?? []}
      disabled={useLive().locked}
      onChange={(v) => props.setValue(v)}
    />
  ),
);

const SelectImpl = createComponentImplementation(
  { name: "Select", schema: z.object({ label: z.string(), options: Options, value: CommonSchemas.DynamicString }) },
  ({ props }) => (
    <SelectView label={props.label} options={props.options} value={props.value ?? ""} disabled={useLive().locked} onChange={(v) => props.setValue(v)} />
  ),
);

const CheckboxImpl = createComponentImplementation(
  { name: "Checkbox", schema: z.object({ label: z.string(), value: CommonSchemas.DynamicBoolean }) },
  ({ props }) => <CheckboxView label={props.label} value={props.value ?? false} disabled={useLive().locked} onChange={(v) => props.setValue(v)} />,
);

const TextInputImpl = createComponentImplementation(
  { name: "TextInput", schema: z.object({ label: z.string(), value: CommonSchemas.DynamicString, hint: z.string().optional() }) },
  ({ props }) => (
    <TextInputView label={props.label} hint={props.hint} value={props.value ?? ""} disabled={useLive().locked} onChange={(v) => props.setValue(v)} />
  ),
);

const AppGateImpl = createComponentImplementation(
  { name: "AppGate", schema: z.object({ slot: z.string(), target: z.object({ event: z.string(), field: z.string() }) }) },
  ({ props }) => {
    const { slots, busy, locked, onPickSlot } = useLive();
    return (
      <AppGateView
        event={props.target.event}
        field={props.target.field}
        path={slots[props.slot]?.path ?? null}
        disabled={busy || locked}
        onPick={() => onPickSlot(props.slot)}
      />
    );
  },
);

const SubmitButtonImpl = createComponentImplementation({ name: "SubmitButton", schema: z.object({}) }, () => {
  const { slots, busy, locked, submit } = useLive();
  if (locked) return null;
  const ready = Object.values(slots).every((s) => s.path);
  return <SubmitButtonView disabled={busy || !ready} needSlots={!ready} onSubmit={submit} />;
});

const CATALOG = new Catalog<ReactCatalogComponent>(CATALOG_ID, "1.0", [
  BlueprintImpl,
  FlowPreviewImpl,
  ChipSelectImpl,
  SelectImpl,
  CheckboxImpl,
  TextInputImpl,
  AppGateImpl,
  SubmitButtonImpl,
]);

/** 설계서 카드 — A2UI v1.0 렌더러는 이 파일에만 들인다. 버전·상태가 바뀌면 표면을 새로 만든다. */
export default function BlueprintSurface({ blueprint, locked, busy, changed, onSubmit, onRetry, onPickSlot, onData }: BlueprintSurfaceProps) {
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
  const retry = status === "submitted" && blueprint.buildPending && onRetry;
  return (
    <div className="flex flex-col gap-2">
      {surface && (
        <LiveContext.Provider
          value={{
            status,
            slots: blueprint.slots,
            locked,
            busy,
            changed: changed ?? [],
            onPickSlot,
            submit: () => onSubmit(structuredClone(surface.dataModel.get("/")) as Record<string, unknown>),
          }}
        >
          <A2uiSurface surface={surface} />
        </LiveContext.Provider>
      )}
      {retry && (
        <button type="button" disabled={busy} onClick={retry} className={`${CARD_BUTTON} self-start`}>
          <RotateCw size={13} aria-hidden="true" />
          {t("chat.blueprint.retry")}
        </button>
      )}
    </div>
  );
}
