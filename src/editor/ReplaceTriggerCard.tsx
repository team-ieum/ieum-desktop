import { t } from "../i18n";
import { BUTTON, PRIMARY } from "./buttons";
import { CARD_BOX } from "./chat/HttpTestCard";
import { useCaptureEscape } from "./useCaptureEscape";

/** 시작 조건을 다른 트리거로 바꾸기 전 확인 — 바꾸면 잃을 수 있는 것만 줄로 보인다. */
export function ReplaceTriggerCard({
  oldName,
  newName,
  refs,
  folder,
  turnsOff,
  onAnswer,
}: {
  oldName: string;
  newName: string;
  refs: number;
  folder: boolean;
  turnsOff: boolean;
  onAnswer: (ok: boolean) => void;
}) {
  // 한 번에 함수로 바꾼다 — 사용자가 지은 이름 속 `$&`·`{new}`가 치환 패턴으로 읽히지 않게.
  const title = t("replace.title").replace(/\{(old|new)\}/g, (_, k) => (k === "old" ? oldName : newName));
  // 카드만 닫는다 — 여백·캔버스를 눌러 포커스가 body로 빠져도 뒤의 설정 패널까지 Esc가 가지 않게.
  useCaptureEscape(() => onAnswer(false));
  return (
    // 여백을 눌러도 포커스가 카드에 머물게 tabIndex -1.
    <div role="dialog" aria-label={title} tabIndex={-1} className={`${CARD_BOX} outline-none`}>
      <h2 className="typo-body2_bold text-neutral-900">{title}</h2>
      {(refs > 0 || folder || turnsOff) && (
        <ul className="flex flex-col gap-1.5 typo-caption1_medium text-neutral-700">
          {refs > 0 && <li>{t("replace.refs").replace("{n}", String(refs))}</li>}
          {folder && <li>{t("replace.folder")}</li>}
          {turnsOff && <li className="typo-caption1_semibold text-danger-700">{t("replace.off")}</li>}
        </ul>
      )}
      <div className="flex justify-end gap-2">
        {/* 처음 포커스는 취소 — Enter 한 번으로 바뀌지 않게. */}
        <button type="button" className={BUTTON} autoFocus onClick={() => onAnswer(false)}>
          {t("common.cancel")}
        </button>
        <button type="button" className={PRIMARY} onClick={() => onAnswer(true)}>
          {t("replace.ok")}
        </button>
      </div>
    </div>
  );
}
