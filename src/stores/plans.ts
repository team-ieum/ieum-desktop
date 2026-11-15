import { useCallback, useEffect } from "react";
import { create } from "zustand";
import { api, type CredentialsView } from "../api";
import { accountsOf, planKey, type Plans, type SubId } from "../screens/settings/AiPane";
import { useWeb } from "./web";

/** 구독 회사마다 기본 계정 사용량 — 상태 줄과 대시보드가 같이 본다. 키 없음 = 읽는 중, null = 못 읽음. */
export const usePlanStore = create<{ plans: Plans }>(() => ({ plans: {} }));
const pending = new Set<string>();

/** 키를 읽는다. 읽는 중인 키는 건너뛰고, force가 아니면 이미 읽은 키도 건너뛴다. fresh = [새로 고침](앱 캐시도 건너뜀). */
function loadPlans(sig: string, force = false, fresh = false) {
  for (const key of sig.split(",")) {
    if (pending.has(key) || (!force && key in usePlanStore.getState().plans)) continue;
    pending.add(key);
    const [k, id] = key.split(":") as [SubId, string];
    (k === "antigravity" ? api.antigravityPlanUsage(fresh) : api.subscriptionUsage(k, id, fresh))
      .catch(() => null)
      .then((u) => {
        pending.delete(key);
        usePlanStore.setState((s) => ({ plans: { ...s.plans, [key]: u } }));
      });
  }
}

/** 감시 루프 결과를 스토어에 합친다 — 앱 전체에서 한 번만. 구독에 실패하면 다음 호출 때 다시 시도. */
let watching: Promise<unknown> | null = null;
function watchPlans() {
  watching ??= api
    .onPlansUpdated((p) => usePlanStore.setState((s) => ({ plans: { ...s.plans, ...p } })))
    .catch(() => {
      watching = null;
    });
}

/** 창이 앞으로 오면(포커스·보이게 됨) 기본 계정 키를 다시 읽는다 — 일반 조회라 앱 캐시 60초는 지킨다.
 *  백그라운드가 만료로 놓친 계정도 여기서 키체인을 다시 읽는다. 리스너는 앱 전체에서 한 번만. */
let focusSig = "";
let focusWatching = false;
function watchFocus(sig: string) {
  focusSig = sig;
  if (focusWatching) return;
  focusWatching = true;
  const again = () => loadPlans(focusSig, true);
  window.addEventListener("focus", again);
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible") again();
  });
}

/** 마지막으로 본 이음 계정 — 바뀌면(로그아웃·다른 계정) 이전 계정의 사용량을 비운다. */
let lastWho: string | undefined;

export function useDefaultPlans(view: CredentialsView | null | undefined) {
  const who = useWeb((s) => s.account?.email ?? "");
  const sig = [
    ...(["claude", "codex"] as const).flatMap((k) =>
      accountsOf(view, k)
        .filter((m) => m.isDefault)
        .map((m) => planKey(k, m.id)),
    ),
    planKey("antigravity"),
  ].join(",");
  const plans = usePlanStore((s) => s.plans);
  useEffect(() => {
    watchPlans();
    watchFocus(sig);
    if (lastWho !== undefined && lastWho !== who) usePlanStore.setState({ plans: {} });
    lastWho = who;
    loadPlans(sig);
  }, [sig, who]);
  const refresh = useCallback(() => loadPlans(sig, true), [sig]);
  const refreshNow = useCallback(() => loadPlans(sig, true, true), [sig]);
  return { plans, refresh, refreshNow };
}
