import { useEffect, useState } from "react";
import { api, type ModelChoice } from "../api";
import { t } from "../i18n";
import type { DropItem } from "../usage/parts";

/** 공급원의 모델 목록. `models` = 읽는 중이면 null, `failed` = 못 읽음(드롭다운 아래 알린다). */
export function useModels(provider: string | undefined): { models: ModelChoice[] | null; failed: boolean } {
  const [got, setGot] = useState<{ provider?: string; models: ModelChoice[] | null; failed: boolean }>({ models: null, failed: false });
  useEffect(() => {
    if (!provider) return;
    let gone = false;
    api.listModels(provider).then(
      (models) => !gone && setGot({ provider, models, failed: false }),
      () => !gone && setGot({ provider, models: [], failed: true }),
    );
    return () => {
      gone = true;
    };
  }, [provider]);
  // 공급원이 바뀐 직후에는 이전 공급원의 목록을 보이지 않는다.
  return got.provider === provider ? got : { models: null, failed: false };
}

/** OpenAI는 기본 모델이 없다 — "기본 모델" 대신 목록 첫 모델을 고른다. */
export const needsModel = (provider: string | undefined) => provider === "api:openai";

/**
 * 드롭다운 항목: "기본 모델"(빈 값, `needsModel`이면 비어 있을 때만 "모델을 고르세요") → 목록 → 저장된 값이 목록에 없으면 "(목록에 없음)"으로 그대로.
 * 읽는 중(`models` null)에는 저장된 값을 표시만 한다.
 */
export function modelItems(provider: string | undefined, models: ModelChoice[] | null, value: string): DropItem<string>[] {
  // 모델이 꼭 필요한데 비어 있으면 "모델을 고르세요" — 첫 모델을 고른 것처럼 보이지 않게(저장본엔 없다).
  const items: DropItem<string>[] = !needsModel(provider)
    ? [{ id: "", name: t("model.default"), icon: null }]
    : value
      ? []
      : [{ id: "", name: t("model.pick"), icon: null }];
  for (const m of models ?? []) items.push({ id: m.id, name: m.label, icon: null });
  if (value && !(models ?? []).some((m) => m.id === value))
    items.push({ id: value, name: models ? `${value} ${t("model.notListed")}` : value, icon: null });
  if (items.length === 0) items.push({ id: "", name: t("model.loading"), icon: null });
  return items;
}
