import { ArrowUp } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { api, type ChatCard } from "../../api";
import symbol from "../../assets/symbol.png";
import { t } from "../../i18n";
import { useCredentials } from "../../stores/credentials";
import { PickFolderCard, TestCard } from "./ChatCards";
import { AI_KEY, aiChoices } from "./chatAi";

type Item = { kind: "user" | "assistant" | "error"; text: string } | { kind: "card"; card: ChatCard };

const FIELD =
  "rounded-[10px] border border-neutral-200 bg-white px-3 py-1.5 typo-caption1_medium text-neutral-700 outline-none focus:border-main-blue";
const ASSISTANT_BUBBLE = "rounded-[0_14px_14px_14px] px-3.5 py-2.5 text-sm leading-relaxed text-neutral-800 max-w-[280px]";

function savedAi(): { value: string; model: string } {
  try {
    const v = JSON.parse(localStorage.getItem(AI_KEY) ?? "{}");
    return { value: typeof v.value === "string" ? v.value : "", model: typeof v.model === "string" ? v.model : "" };
  } catch {
    return { value: "", model: "" };
  }
}

const Face = () => (
  <span className="grid h-7 w-7 shrink-0 place-items-center rounded-lg" style={{ background: "#e0f6ff" }}>
    <img src={symbol} alt="" className="h-4 w-4 object-contain" />
  </span>
);

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
  const loaded = useRef(false);

  const choice = choices.find((c) => c.value === ai.value) ?? choices[0];
  const needsModel = choice?.provider === "api:openai" && !ai.model.trim();

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

  const pickAi = (next: { value: string; model: string }) => {
    setAi(next);
    try {
      localStorage.setItem(AI_KEY, JSON.stringify(next));
    } catch {
      // 저장하지 못해도 이번 선택은 그대로 쓴다.
    }
  };

  const send = async (message: string) => {
    const said = message.trim();
    if (!said || busy || !choice || needsModel) return;
    setBusy(true);
    setStage(null);
    setItems((now) => [...now, { kind: "user", text: said }]);
    try {
      const turn = await api.chatSend(workflowId, said, {
        provider: choice.provider,
        credential: choice.credential || null,
        model: ai.model.trim() || null,
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
    if (!message.trim() || busy || !choice || needsModel) return;
    setText("");
    await send(message);
  };

  return (
    <div data-slot="chat" hidden={!visible} className={`min-h-0 flex-1 flex-col ${visible ? "flex" : ""}`}>
      <div ref={body} className="flex flex-1 flex-col gap-3 overflow-y-auto p-4">
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
              <div className="max-w-[280px] rounded-[14px_14px_4px_14px] bg-main-blue px-3.5 py-2.5 text-sm leading-relaxed whitespace-pre-wrap text-white">
                {item.text}
              </div>
            </div>
          ) : item.kind === "assistant" ? (
            <div key={i} className="flex items-start gap-2">
              <Face />
              <div className={`${ASSISTANT_BUBBLE} whitespace-pre-wrap`} style={{ background: "#F0F4FC" }}>
                {item.text}
              </div>
            </div>
          ) : (
            <p key={i} role="alert" className="typo-caption1_medium text-danger-700">
              {item.text}
            </p>
          ),
        )}
        {busy && (
          <div className="flex items-start gap-2">
            <Face />
            <div className="flex max-w-[280px] flex-wrap items-center gap-2 rounded-[0_14px_14px_14px] px-4 py-3.5" style={{ background: "#F0F4FC" }}>
              <div className="flex items-center gap-1.5">
                {[0, 180, 360].map((delay) => (
                  <span key={delay} className="block h-1.5 w-1.5 animate-pulse rounded-full bg-main-blue" style={{ animationDelay: `${delay}ms` }} />
                ))}
              </div>
              {stage && <span className="typo-caption1_medium leading-none text-main-blue">{t(`chat.stage.${stage}`)}</span>}
            </div>
          </div>
        )}
      </div>
      <div className="flex shrink-0 flex-col gap-2 border-t border-neutral-200 bg-neutral-50 p-3">
        {choice ? (
          <>
            <div className="flex gap-2">
              <select
                aria-label={t("chat.ai")}
                value={choice.value}
                onChange={(e) => pickAi({ ...ai, value: e.target.value })}
                className={`${FIELD} min-w-0 flex-1`}
              >
                {choices.map((c) => (
                  <option key={c.value} value={c.value}>
                    {c.label}
                  </option>
                ))}
              </select>
              <input
                aria-label={t("chat.model")}
                placeholder={t("chat.modelDefault")}
                value={ai.model}
                onChange={(e) => pickAi({ value: choice.value, model: e.target.value })}
                className={`${FIELD} w-32`}
              />
            </div>
            {needsModel && <p className="typo-caption1_medium text-danger-700">{t("chat.modelRequired")}</p>}
          </>
        ) : (
          <p role="note" className="typo-caption1_medium text-neutral-600">
            {t("chat.noAi")}
          </p>
        )}
        <div className="flex items-center gap-2 rounded-[14px] border border-neutral-200 bg-white px-3 py-2">
          <input
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.nativeEvent.isComposing) void submit();
            }}
            aria-label={t("chat.input")}
            placeholder={t("chat.placeholder")}
            disabled={busy || !choice}
            className="flex-1 bg-transparent text-sm outline-none"
          />
          <button
            type="button"
            onClick={() => void submit()}
            disabled={busy || !choice || !text.trim()}
            aria-label={t("chat.send")}
            className="grid h-7 w-7 shrink-0 cursor-pointer place-items-center rounded-[10px] bg-main-deep-blue disabled:opacity-40"
          >
            <ArrowUp size={14} className="text-white" aria-hidden="true" />
          </button>
        </div>
      </div>
    </div>
  );
}
