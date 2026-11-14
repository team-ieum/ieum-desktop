import { ArrowUp, Gauge, Sparkles } from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { api, type ChatCard } from "../../api";
import { ProviderLogo } from "../../assets/providers";
import { type Key, t } from "../../i18n";
import { useCredentials } from "../../stores/credentials";
import { Dropdown } from "../../usage/parts";
import { effortItems, effortLevels, modelItems, needsModel, shownModel, useModels } from "../models";
import { PickFolderCard, TestCard } from "./ChatCards";
import { AI_KEY, aiChoices } from "./chatAi";

type Item = { kind: "user" | "assistant" | "error"; text: string } | { kind: "card"; card: ChatCard };

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

/** 채팅 패널 본문. 닫혀도 언마운트되지 않고 `visible`만 바뀐다. 카드는 저장되지 않아 다시 열면 말풍선만 복원된다. */
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
  // 앱이 켜 두었고 설치된 구독 CLI — Codex·Antigravity 선택지를 여는 조건.
  const [clis, setClis] = useState<string[]>([]);
  const choices = aiChoices(view, clis);
  const [items, setItems] = useState<Item[]>([]);
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [stage, setStage] = useState<"designing" | "testing" | null>(null);
  const [ai, setAi] = useState(savedAi);
  const body = useRef<HTMLDivElement>(null);
  const box = useRef<HTMLTextAreaElement>(null);
  const loaded = useRef(false);

  const choice = choices.find((c) => c.value === ai.value) ?? choices[0];
  const got = useModels(choice?.provider);
  const { models, failed } = got;
  // 예전 Antigravity 값(`…-high`)은 모델·수준으로 나눠 보여 준다.
  const shown = shownModel(choice?.provider, ai.model, ai.effort);
  const effort = effortLevels(choice?.provider, got, shown.model);
  const missingModel = needsModel(choice?.provider) && !ai.model.trim();
  const ready = !!choice && !busy && !missingModel;

  useEffect(() => {
    if (!visible || loaded.current) return;
    loaded.current = true;
    // 실패는 삼키고 빈 대화로 그린다.
    api.getChat(workflowId).then(
      (c) => setItems((now) => [...c.messages.map((m) => ({ kind: m.role, text: m.text })), ...now]),
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
        () => {},
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
  useEffect(() => {
    if (choice && missingModel && models?.length) pickAi({ value: choice.value, model: models[0].id, effort: "" });
  }, [choice?.value, missingModel, models]);

  const send = async (message: string) => {
    const said = message.trim();
    if (!said || !ready || !choice) return;
    setBusy(true);
    setStage(null);
    setItems((now) => [...now, { kind: "user", text: said }]);
    try {
      const turn = await api.chatSend(workflowId, said, {
        provider: choice.provider,
        credential: choice.credential || null,
        model: ai.model.trim() || null,
        effort: ai.effort || null,
      });
      setItems((now) => [
        ...now,
        ...(turn.say ? [{ kind: "assistant" as const, text: turn.say }] : []),
        // 완료 확인 카드는 캔버스 위에 따로 뜬다 — 채팅 안에는 그리지 않는다.
        ...turn.cards.filter((c) => c.kind !== "finish").map((card) => ({ kind: "card" as const, card })),
        ...(turn.error ? [{ kind: "error" as const, text: turn.error }] : []),
      ]);
    } catch (e) {
      setItems((now) => [...now, { kind: "error", text: t(e === "CHAT_BUSY" ? "chat.error.busy" : "chat.error.failed") }]);
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

  return (
    <div data-slot="chat" hidden={!visible} className={`min-h-0 flex-1 flex-col ${visible ? "flex" : ""}`}>
      <div ref={body} className="flex flex-1 flex-col gap-3 overflow-y-auto p-4">
        {items.length === 0 && !busy && <Empty onPick={(s) => void send(s)} disabled={!ready} />}
        {items.map((item, i) =>
          item.kind === "card" ? (
            <div key={i} className="pl-9">
              {item.card.kind === "test" ? (
                <TestCard runId={item.card.runId} status={item.card.status} nodeNames={nodeNames} />
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
