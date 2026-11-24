import { t } from "../i18n";
import { BUTTON, PRIMARY } from "./buttons";
import { CARD_BOX } from "./chat/HttpTestCard";

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
  const title = t("replace.title").replace("{old}", oldName).replace("{new}", newName);
  return (
    <div
      role="dialog"
      aria-label={title}
      className={CARD_BOX}
      onKeyDown={(e) => {
        if (e.key !== "Escape") return;
        // 카드만 닫는다 — 뒤의 설정 패널까지 Esc가 가지 않게.
        e.stopPropagation();
        onAnswer(false);
      }}
    >
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
