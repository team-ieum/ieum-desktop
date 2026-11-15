import { useEffect, useState } from "react";
import { api, type ModelChoice } from "../api";
import { type Key, t } from "../i18n";
import type { DropItem } from "../usage/parts";

type Got = { models: ModelChoice[] | null; defaultEfforts: string[]; failed: boolean };
const LOADING: Got = { models: null, defaultEfforts: [], failed: false };

/** 공급원의 모델 목록. `models` = 읽는 중이면 null, `failed` = 못 읽음(드롭다운 아래 알린다). */
export function useModels(provider: string | undefined): Got {
  const [got, setGot] = useState<Got & { provider?: string }>(LOADING);
  useEffect(() => {
    if (!provider) return;
    let gone = false;
    api.listModels(provider).then(
      (l) => !gone && setGot({ provider, models: l.models, defaultEfforts: l.defaultEfforts, failed: false }),
      () => !gone && setGot({ provider, models: [], defaultEfforts: [], failed: true }),
    );
    return () => {
      gone = true;
    };
  }, [provider]);
  // 공급원이 바뀐 직후에는 이전 공급원의 목록을 보이지 않는다.
  return got.provider === provider ? got : LOADING;
}

/** OpenAI는 기본 모델이 없다 — "기본 모델" 대신 목록 첫 모델을 고른다. */
export const needsModel = (provider: string | undefined) => provider === "api:openai";

const AGY = "cli:agy";
const AGY_EFFORTS = ["xhigh", "high", "medium", "low"];

/**
 * 저장된 (모델, 수준) → 화면에 보일 (모델, 수준). Antigravity 예전 저장값은 수준이 id에 붙어 있다(`gemini-4-argon-high`) —
 * 수준을 따로 고르지 않았으면 `gemini-4-argon` + `high`로 나눠 보여 준다. 저장본은 사용자가 바꾸기 전엔 그대로.
 */
export function shownModel(provider: string | undefined, model: string, effort: string): { model: string; effort: string } {
  if (provider !== AGY) return { model, effort };
  const e = AGY_EFFORTS.find((x) => model.endsWith(`-${x}`) && model.length > x.length + 1);
  return e ? { model: model.slice(0, -(e.length + 1)), effort: effort || e } : { model, effort };
}

/**
 * 드롭다운 항목: "기본 모델"(빈 값, `needsModel`이면 비어 있을 때만 "모델을 고르세요") → 목록 → 저장된 값이 목록에 없으면 "(목록에 없음)"으로 그대로.
 * 읽는 중(`models` null)에는 저장된 값을 표시만 한다. `value`는 `shownModel`을 거친 값.
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

/** 수준 화면 이름 — 공급원 문서의 이름 그대로(`xhigh` → XHigh). */
export const effortName = (e: string) => (e === "xhigh" ? "XHigh" : e.charAt(0).toUpperCase() + e.slice(1));

/**
 * 고른 공급원·모델(`shownModel`을 거친 값)이 받는 수준과, 못 고르면 그 이유.
 * 기본 모델이면 공급원의 기본 모델 수준(`defaultEfforts` — claude CLI처럼 모델과 무관한 수준 포함).
 */
export function effortLevels(
  provider: string | undefined,
  got: Got,
  model: string,
): { levels: string[]; off: Key | null } {
  if (!got.models) return { levels: [], off: "model.loading" };
  if (!model) {
    if (got.defaultEfforts.length) return { levels: got.defaultEfforts, off: null };
    return { levels: [], off: provider === AGY ? "effort.off.agyDefault" : "effort.off.provider" };
  }
  const levels = got.models.find((m) => m.id === model)?.efforts ?? [];
  return { levels, off: levels.length ? null : "effort.off.model" };
}

/** 수준 드롭다운 항목: "기본"(빈 값) → 받는 수준 → 저장된 값이 목록에 없으면 "(목록에 없음)"으로 그대로. */
export function effortItems(levels: string[], value: string): DropItem<string>[] {
  const items: DropItem<string>[] = [{ id: "", name: t("effort.default"), icon: null }];
  for (const e of levels) items.push({ id: e, name: effortName(e), icon: null });
  if (value && !levels.includes(value)) items.push({ id: value, name: `${effortName(value)} ${t("model.notListed")}`, icon: null });
  return items;
}
