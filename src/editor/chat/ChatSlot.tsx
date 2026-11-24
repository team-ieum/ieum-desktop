import { ArrowDown, ArrowUp, CircleAlert, Gauge, Sparkles } from "lucide-react";
import { Component, lazy, type ReactNode, Suspense, useEffect, useLayoutEffect, useRef, useState } from "react";
import { api, type Blueprint, type ChatCard } from "../../api";
import { ProviderLogo } from "../../assets/providers";
import { type Key, t } from "../../i18n";
import { useCredentials } from "../../stores/credentials";
import { useSettings } from "../../stores/settings";
import { Dropdown } from "../../usage/parts";
import { effortItems, effortLevels, modelItems, needsModel, shownModel, useModels } from "../models";
import { PickFolderCard, TestCard } from "./ChatCards";
import { AI_KEY, aiChoices } from "./chatAi";

// 설계서 래퍼(@a2ui/*)는 별도 청크 — 설계서가 없으면 불러오지 않는다.
const BlueprintSurface = lazy(() => import("./a2ui/BlueprintSurface"));

/** `blueprint`는 지금 설계서(하나뿐), `blueprintStub`은 AI가 고쳐 아래로 옮긴 옛 자리. */
type Item =
  | { kind: "user" | "assistant" | "error"; text: string }
  | { kind: "card"; card: ChatCard }
  | { kind: "blueprint" }
  | { kind: "blueprintStub"; title: string };

const titleOf = (b: Blueprint) => String(b.components.find((c) => c.id === "root")?.title ?? "");
const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);
/** JSON Pointer 위치의 값. 없으면 undefined. */
const at = (doc: unknown, pointer: string) =>
  pointer
    .split("/")
    .slice(1)
    .reduce<unknown>((v, k) => (v !== null && typeof v === "object" ? (v as Record<string, unknown>)[k.replace(/~1/g, "/").replace(/~0/g, "~")] : undefined), doc);
/** 컴포넌트가 묶인 데이터 경로(`{"@path": p}` prop). */
const boundPaths = (c: Record<string, unknown>) =>
  Object.values(c).flatMap((v) => {
    const p = (v as { "@path"?: unknown } | null)?.["@path"];
    return typeof p === "string" ? [p] : [];
  });
/** AI가 고친 컴포넌트 — 같은 id의 JSON이 달라졌거나 새로 생겼거나, 묶인 값이 바뀐 것. */
export const changedIds = (old: Blueprint, next: Blueprint) =>
  next.components
    .filter((c) => !same(c, old.components.find((o) => o.id === c.id)) || boundPaths(c).some((p) => !same(at(old.dataModel, p), at(next.dataModel, p))))
    .map((c) => String(c.id));
const failText = (e: unknown) => t(e === "CHAT_BUSY" ? "chat.error.busy" : "chat.error.failed");

const BUBBLE = "max-w-[280px] px-3.5 py-2.5 text-sm leading-relaxed whitespace-pre-wrap";
const ASSISTANT_BG = { background: "#F0F4FC" };
/** 입력 카드 아래 줄의 고르기 칩. */
const CHIP =
  "inline-flex h-7 min-w-0 max-w-full cursor-pointer items-center gap-1 rounded-full border border-[#cde9f4] bg-main-light-blue/40 px-2.5 typo-caption2_semibold whitespace-nowrap text-main-deep-blue hover:bg-main-light-blue disabled:cursor-default disabled:opacity-50";
const EXAMPLES: Key[] = ["chat.example.1", "chat.example.2", "chat.example.3"];
/** 입력칸이 자라는 최대 높이(px) — 넘으면 안에서 스크롤. */
const MAX_INPUT = 160;

type Ai = { value: string; model: string; effort: string };

function savedAi(): Ai {
  const str = (x: unknown) => (typeof x === "string" ? x : "");
  try {
    const v = JSON.parse(localStorage.getItem(AI_KEY) ?? "{}");
    return { value: str(v.value), model: str(v.model), effort: str(v.effort) };
  } catch {
    return { value: "", model: "", effort: "" };
  }
}

const Face = () => <ProviderLogo id="ieum" size={28} />;

/** 설계서 카드가 그리다 죽어도(저장본에 남은 이상한 값 등) 채팅은 산다 — 그 자리만 한 줄로 바꾼다. */
class BlueprintGuard extends Component<{ children: ReactNode }, { broken: boolean }> {
  state = { broken: false };
  static getDerivedStateFromError() {
    return { broken: true };
  }
  render() {
    if (!this.state.broken) return this.props.children;
    return (
      <p className="flex items-center gap-1.5 typo-caption1_medium text-neutral-500">
        <CircleAlert size={13} className="shrink-0" aria-hidden="true" />
        {t("chat.blueprint.broken")}
      </p>
    );
  }
}

/** 응답을 기다리는 동안 — 웹 이음과 같은 세 점 + 단계 글. */
function Typing({ stage }: { stage: "designing" | "testing" | null }) {
  return (
    <div className="flex items-start gap-2">
      <Face />
      <div className="flex max-w-[280px] flex-wrap items-center gap-2 rounded-[0_14px_14px_14px] px-4 py-3.5" style={ASSISTANT_BG}>
        <div className="flex items-center gap-1.5" aria-hidden="true">
          {[0, 180, 360].map((delay) => (
            <span key={delay} className="ieum-dot block h-1.5 w-1.5 rounded-full bg-main-blue" style={{ animationDelay: `${delay}ms` }} />
          ))}
        </div>
        {stage && <span className="typo-caption1_medium leading-none text-main-blue">{t(`chat.stage.${stage}`)}</span>}
      </div>
    </div>
  );
}

/** 대화가 비었을 때 — 마스코트, 인사, 눌러서 바로 보내는 예시. */
function Empty({ onPick, disabled }: { onPick: (text: string) => void; disabled: boolean }) {
  return (
    <div className="m-auto flex w-full flex-col items-center gap-4 px-2 py-6 text-center">
      <span className="grid rounded-2xl shadow-[0_8px_20px_-8px_rgba(0,123,167,.45)]">
        <ProviderLogo id="ieum" size={56} />
      </span>
      <div className="flex flex-col gap-1">
        <p className="typo-body1_bold text-main-deep-blue">{t("chat.empty.title")}</p>
        <p className="typo-caption1_medium text-neutral-500">{t("chat.empty.hint")}</p>
      </div>
      <ul aria-label={t("chat.examples")} className="flex w-full flex-col gap-2">
        {EXAMPLES.map((k) => (
          <li key={k}>
            <button
              type="button"
              disabled={disabled}
              onClick={() => onPick(t(k))}
              className="flex w-full cursor-pointer items-start gap-2 rounded-xl border border-[#cde9f4] bg-white px-3 py-2.5 text-left typo-caption1_medium text-neutral-700 transition-colors hover:border-main-blue hover:bg-main-light-blue/30 disabled:cursor-default disabled:opacity-50"
            >
              <Sparkles size={14} className="mt-0.5 shrink-0 text-main-blue" aria-hidden="true" />
              {t(k)}
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}

/** 채팅 패널 본문. 닫혀도 언마운트되지 않고 `visible`만 바뀐다. 카드는 저장되지 않아 다시 열면 말풍선과 설계서만 복원된다. */
export function ChatSlot({
  workflowId,
  visible,
  nodeNames,
}: {
  workflowId: string;
  visible: boolean;
  /** 노드 id → 이름(테스트 결과 카드용). 없으면 카드에 상태만 보인다. */
  nodeNames?: Record<string, string>;
}) {
  const view = useCredentials((s) => s.view);
  // 앱이 켜 두었고 설치된 구독 CLI — Codex·Antigravity 선택지를 여는 조건. null = 아직 감지 전.
  const [clis, setClis] = useState<string[] | null>(null);
  const choices = aiChoices(view, clis ?? []);
  const [items, setItems] = useState<Item[]>([]);
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [stage, setStage] = useState<"designing" | "testing" | null>(null);
  const [ai, setAi] = useState(savedAi);
  const body = useRef<HTMLDivElement>(null);
  const box = useRef<HTMLTextAreaElement>(null);
  const loaded = useRef(false);
  const modeOn = useSettings((s) => s.settings?.chatBlueprint === true);
  // 저장본 설계서(검증 통과분)와 이번에 AI가 고친 컴포넌트.
  const [bp, setBp] = useState<Blueprint | null>(null);
  const [changed, setChanged] = useState<string[]>([]);
  // 폴더 칸 고르기·제출 중 — 카드만 막는다.
  const [bpBusy, setBpBusy] = useState(false);
  // 카드에서 고친 데이터 모델(null = 저장본 그대로)과 마지막 보내기 뒤 고쳤는지.
  const dataRef = useRef<Record<string, unknown> | null>(null);
  const touchedRef = useRef(false);

  const matched = choices.find((c) => c.value === ai.value);
  const choice = matched ?? choices[0];
  // 저장한 선택지가 없어 대체했으면 저장한 모델·수준은 다른 공급원 것이다 — 쓰지 않는다.
  const model = matched ? ai.model : "";
  const aiEffort = matched ? ai.effort : "";
  const got = useModels(choice?.provider);
  const { models, failed } = got;
  // 예전 Antigravity 값(`…-high`)은 모델·수준으로 나눠 보여 준다.
  const shown = shownModel(choice?.provider, model, aiEffort);
  const effort = effortLevels(choice?.provider, got, shown.model);
  const missingModel = needsModel(choice?.provider) && !model.trim();
  const ready = !!choice && !busy && !missingModel;

  useEffect(() => {
    if (!visible || loaded.current) return;
    loaded.current = true;
    // 실패는 삼키고 빈 대화로 그린다.
    api.getChat(workflowId).then(
      (c) => {
        const b = c.state.blueprint ?? null;
        setBp(b);
        setItems((now) => [...c.messages.map((m): Item => ({ kind: m.role, text: m.text })), ...(b ? [{ kind: "blueprint" as const }] : []), ...now]);
      },
      () => {},
    );
    useCredentials
      .getState()
      .load()
      .catch(() => {});
    Promise.resolve()
      .then(() => api.detectProviders())
      .then(
        (p) => setClis(p.clis.map((c) => c.id)),
        // 감지 실패면 구독 CLI 없이 — null로 두면 첫 모델 자동 고르기가 영영 안 돈다.
        () => setClis([]),
      );
  }, [visible, workflowId]);

  useEffect(() => {
    let off = () => {};
    let gone = false;
    api.onChatStage((p) => p.id === workflowId && setStage(p.stage)).then(
      (un) => (gone ? un() : (off = un)),
      () => {},
    );
    return () => {
      gone = true;
      off();
    };
  }, [workflowId]);

  useEffect(() => {
    if (body.current) body.current.scrollTop = body.current.scrollHeight;
  }, [items, busy, stage]);

  // 글에 맞춰 입력칸 높이를 바꾼다.
  useLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, MAX_INPUT)}px`;
  }, [text]);

  const pickAi = (next: Ai) => {
    setAi(next);
    try {
      localStorage.setItem(AI_KEY, JSON.stringify(next));
    } catch {
      // 저장하지 못해도 이번 선택은 그대로 쓴다.
    }
  };

  // OpenAI는 기본 모델이 없다 — 비어 있으면 목록 첫 모델을 골라 둔다.
  // CLI 감지 전에는 고르지 않는다 — 저장한 Codex·Antigravity 선택을 대체 선택지로 덮어쓰게 된다.
  useEffect(() => {
    if (clis !== null && choice && missingModel && models?.length) pickAi({ value: choice.value, model: models[0].id, effort: "" });
  }, [clis, choice?.value, missingModel, models]);

  const addError = (text: string) => setItems((now) => [...now, { kind: "error", text }]);
  /** 저장본 설계서를 다시 읽는다. 읽지 못하면 undefined(지금 것을 그대로 둔다). */
  const reread = () => api.getChat(workflowId).then((c) => c.state.blueprint ?? null, () => undefined);
  const showBp = (before: Blueprint | null, next: Blueprint | null | undefined, moved = false) => {
    if (next === undefined) return;
    // 판·상태가 바뀌면 래퍼가 표면을 저장본으로 새로 만든다 — 카드에서 고친 값·고쳤는지를 버리고 강조도 새로 정한다.
    // 같은 판이면 표면이 그대로라 강조를 건드리지 않는다(켜진 강조를 2초 안에 끄면 일찍 꺼진다).
    if (next?.version !== before?.version || next?.status !== before?.status) {
      dataRef.current = null;
      touchedRef.current = false;
      setChanged(moved && before && next ? changedIds(before, next) : []);
    }
    setBp(next);
  };

  /** `noBlueprint` = 제출 직후·이어서 만들기 — 상태값의 설계서가 아직 active여도 데이터를 싣지 않는다. */
  const send = async (message: string, opts?: { noBlueprint?: boolean }) => {
    const said = message.trim();
    if (!said || !ready || !choice) return;
    setBusy(true);
    setStage(null);
    setItems((now) => [...now, { kind: "user", text: said }]);
    const before = bp;
    // 떠 있던·만들던 설계서는 턴이 어떻게 끝나든 다시 읽는다 — 만들기 완료·우회 취소는 카드 없이 온다.
    const sync = before?.status === "active" || (before?.status === "submitted" && before.buildPending);
    const carry = before?.status === "active" && modeOn && !opts?.noBlueprint;
    const sent = carry ? (dataRef.current ?? before.dataModel) : null;
    const sendAi = { provider: choice.provider, credential: choice.credential || null, model: model.trim() || null, effort: aiEffort || null };
    try {
      const turn = sent ? await api.chatSend(workflowId, said, sendAi, { data: sent, touched: touchedRef.current }) : await api.chatSend(workflowId, said, sendAi);
      if (carry) touchedRef.current = false;
      const moved = turn.cards.some((c) => c.kind === "blueprint");
      // 강조는 보낸 데이터와 비교한다 — 사람이 카드에서 고친 칸을 AI가 바꾼 것으로 보이지 않게.
      if (moved || sync) showBp(before && sent ? { ...before, dataModel: sent } : before, await reread(), moved);
      setItems((now) => [
        // AI가 고친 설계서는 맨 아래로 — 옛 자리에는 한 줄만 남긴다.
        ...(moved ? now.flatMap((i): Item[] => (i.kind !== "blueprint" ? [i] : before ? [{ kind: "blueprintStub", title: titleOf(before) }] : [])) : now),
        ...(turn.say ? [{ kind: "assistant" as const, text: turn.say }] : []),
        // 완료 확인 카드는 캔버스 위에 따로 뜬다 — 채팅 안에는 그리지 않는다.
        ...turn.cards.filter((c) => c.kind !== "finish" && c.kind !== "blueprint").map((card) => ({ kind: "card" as const, card })),
        ...(moved ? [{ kind: "blueprint" as const }] : []),
        ...(turn.error ? [{ kind: "error" as const, text: turn.error }] : []),
      ]);
    } catch (e) {
      if (sync) showBp(before, await reread());
      addError(failText(e));
    } finally {
      setBusy(false);
      setStage(null);
    }
  };
  const submit = async () => {
    const message = text;
    if (!message.trim() || !ready) return;
    setText("");
    await send(message);
  };
  const sendBuild = () => void send(t("chat.blueprint.submitText"), { noBlueprint: true });

  const submitBp = async (data: Record<string, unknown>) => {
    if (!bp) return;
    setBpBusy(true);
    let ok = false;
    try {
      const r = await api.chatSubmitBlueprint(workflowId, bp.version, data);
      ok = r.ok;
      if (ok) showBp(bp, await reread());
      else {
        addError(r.message || t("chat.error.failed"));
        // 그사이 판이 바뀌었거나 취소됐다 — 지금 판을 다시 그려 같은 거절이 반복되지 않게.
        if (r.code === "STALE") showBp(bp, await reread());
      }
    } catch (e) {
      addError(failText(e));
    } finally {
      setBpBusy(false);
    }
    if (ok) sendBuild();
  };
  const pickSlot = async (slot: string) => {
    if (!bp) return;
    setBpBusy(true);
    try {
      const r = await api.chatPickSlot(workflowId, slot, bp.version);
      if (r.ok) {
        showBp(bp, await reread());
        // 폴더 칸을 고른 것도 카드를 만진 것이다(실측 blueprint_ignored).
        touchedRef.current = true;
      } else if (r.code !== "PICK_CANCELLED") {
        addError(r.message || t("chat.error.failed"));
        if (r.code === "STALE") showBp(bp, await reread());
      }
    } catch (e) {
      addError(failText(e));
    } finally {
      setBpBusy(false);
    }
  };

  return (
    <div data-slot="chat" hidden={!visible} className={`min-h-0 flex-1 flex-col ${visible ? "flex" : ""}`}>
      <div ref={body} className="flex flex-1 flex-col gap-3 overflow-y-auto p-4">
        {items.length === 0 && !busy && <Empty onPick={(s) => void send(s)} disabled={!ready} />}
        {items.map((item, i) =>
          item.kind === "card" ? (
            <div key={i} className="pl-9">
              {item.card.kind === "test" ? (
                <TestCard runId={item.card.runId} status={item.card.status} nodeNames={nodeNames} httpSent={item.card.httpSent} />
              ) : item.card.kind === "pickFolder" ? (
                <PickFolderCard
                  workflowId={workflowId}
                  nodeId={item.card.nodeId}
                  field={item.card.field}
                  disabled={busy}
                  onPicked={() => void send(t("chat.folderPicked"))}
                />
              ) : null}
            </div>
          ) : item.kind === "blueprint" ? (
            bp && (
              <div key={i} className="pl-9">
                <BlueprintGuard>
                  <Suspense fallback={null}>
                    <BlueprintSurface
                      blueprint={bp}
                      locked={!modeOn || bp.status !== "active"}
                      busy={busy || bpBusy}
                      changed={changed}
                      onSubmit={(data) => void submitBp(data)}
                      // 토글이 꺼지면 만들기 턴이 아니다 — [이어서 만들기]를 그리지 않는다.
                      onRetry={modeOn ? sendBuild : undefined}
                      onPickSlot={(slot) => void pickSlot(slot)}
                      onData={(data) => {
                        dataRef.current = data;
                        touchedRef.current = true;
                      }}
                    />
                  </Suspense>
                </BlueprintGuard>
              </div>
            )
          ) : item.kind === "blueprintStub" ? (
            <p key={i} className="ml-9 flex items-center gap-1.5 typo-caption1_medium text-neutral-500">
              <ArrowDown size={13} className="shrink-0" aria-hidden="true" />
              {t("chat.blueprint.moved").replace("{title}", () => item.title)}
            </p>
          ) : item.kind === "user" ? (
            <div key={i} className="flex justify-end">
              <div className={`${BUBBLE} rounded-[14px_14px_4px_14px] bg-main-blue text-white`}>{item.text}</div>
            </div>
          ) : item.kind === "assistant" ? (
            <div key={i} className="flex items-start gap-2">
              <Face />
              <div className={`${BUBBLE} rounded-[0_14px_14px_14px] text-neutral-800`} style={ASSISTANT_BG}>
                {item.text}
              </div>
            </div>
          ) : (
            <p key={i} role="alert" className="ml-9 rounded-xl border border-danger-300 bg-danger-100 px-3 py-2 typo-caption1_medium text-danger-700">
              {item.text}
            </p>
          ),
        )}
        {busy && <Typing stage={stage} />}
      </div>
      <div className="flex shrink-0 flex-col gap-1.5 border-t border-[#e3eef4] bg-[#f7fafd] p-3">
        {!choice && (
          <p role="note" className="px-1 typo-caption1_medium text-neutral-600">
            {t("chat.noAi")}
          </p>
        )}
        <div className="flex flex-col gap-2 rounded-2xl border border-[#d4e4ec] bg-white px-3 pt-2.5 pb-2 shadow-[0_4px_14px_-8px_rgba(41,83,124,.25)] transition-colors focus-within:border-main-blue">
          <textarea
            ref={box}
            rows={1}
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              // Enter = 보내기, Shift+Enter = 줄바꿈. 한글 조합 중 Enter는 글자 확정이라 무시 —
              // WKWebView는 조합 확정 Enter를 isComposing 없이 keyCode 229로 보내기도 한다.
              if (e.key !== "Enter" || e.shiftKey || e.nativeEvent.isComposing || e.keyCode === 229) return;
              e.preventDefault();
              void submit();
            }}
            aria-label={t("chat.input")}
            placeholder={t("chat.placeholder")}
            disabled={busy || !choice}
            className="max-h-40 min-h-6 w-full resize-none bg-transparent text-sm leading-6 text-neutral-800 outline-none placeholder:text-neutral-400 disabled:cursor-not-allowed"
          />
          <div className="flex items-center gap-1.5">
            {choice && (
              <>
                <Dropdown
                  label={t("chat.ai")}
                  items={choices.map((c) => ({ id: c.value, name: c.label, icon: null, group: t(c.provider.startsWith("api:") ? "chat.group.api" : "chat.group.sub") }))}
                  sel={choice.value}
                  onSel={(value) => value !== choice.value && pickAi({ value, model: "", effort: "" })}
                  up
                  left
                  // 칩 셋이 한 줄에 들게 — 좁으면 키 칩이 먼저 줄고 글은 말줄임.
                  root="relative min-w-0 max-w-[150px] shrink-[3]"
                  trigger={CHIP}
                  disabled={busy}
                />
                <Dropdown
                  label={t("chat.model")}
                  items={modelItems(choice.provider, models, shown.model)}
                  sel={shown.model}
                  // 모델마다 받는 수준이 달라 모델을 바꾸면 수준은 기본으로.
                  onSel={(model) => model !== shown.model && pickAi({ value: choice.value, model, effort: "" })}
                  up
                  left
                  root="relative min-w-0 max-w-[150px]"
                  trigger={CHIP}
                  disabled={busy}
                />
                <Dropdown
                  label={t("effort.label")}
                  items={effortItems(effort.levels, shown.effort).map((i) => ({ ...i, icon: <Gauge size={12} aria-hidden="true" className="shrink-0" /> }))}
                  sel={shown.effort}
                  onSel={(e) => pickAi({ value: choice.value, model: shown.model, effort: e })}
                  up
                  root="relative shrink-0"
                  trigger={CHIP}
                  disabled={busy || !!effort.off}
                  title={effort.off ? t(effort.off) : undefined}
                />
              </>
            )}
            <button
              type="button"
              onClick={() => void submit()}
              disabled={!ready || !text.trim()}
              aria-label={t("chat.send")}
              className="ml-auto grid h-8 w-8 shrink-0 cursor-pointer place-items-center rounded-[10px] bg-main-deep-blue transition-opacity hover:opacity-90 disabled:cursor-default disabled:opacity-35"
            >
              <ArrowUp size={15} className="text-white" aria-hidden="true" />
            </button>
          </div>
        </div>
        {missingModel && <p className="px-1 typo-caption2_medium text-danger-700">{t("chat.modelRequired")}</p>}
        {failed && (
          <p role="status" className="px-1 typo-caption2_medium text-danger-700">
            {t("model.listFailed")}
          </p>
        )}
      </div>
    </div>
  );
}
