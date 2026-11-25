import { useEffect, useSyncExternalStore } from "react";
import { api, type OptionError, type OptionList } from "../api";
import type { Field } from "../nodes/defs";

/** 목록에서 고르는 칸으로 그리는 카탈로그 공급원. */
export const GITHUB_SOURCES = ["github.owners", "github.repos", "github.branches", "github.milestones"] as const;

export type OptionsState =
  | { kind: "ready"; inputs: Record<string, string> }
  | { kind: "blocked"; reason: "empty" | "reference" | "invalid"; title: string };

// 앱의 `is_login`·`is_path_name`과 같은 규칙.
const LOGIN = /^[A-Za-z0-9_-]{1,39}$/;
const PATH_NAME = /^[A-Za-z0-9_.-]+$/;
const valid = (key: string, v: string) => (key === "owner" ? LOGIN.test(v) : key === "repo" ? PATH_NAME.test(v) && !/^\.+$/.test(v) : true);

/** 의존 칸(`optionsInputs`) 값이 목록을 부를 수 있는지 — 값은 앞뒤 공백을 자른다. */
export function optionsState(field: Field, config: Record<string, unknown>, fields: Field[]): OptionsState {
  const inputs: Record<string, string> = {};
  for (const key of field.optionsInputs ?? []) {
    const v = String(config[key] ?? "").trim();
    const title = fields.find((f) => f.key === key)?.title ?? key;
    if (!v) return { kind: "blocked", reason: "empty", title };
    if (v.includes("{{")) return { kind: "blocked", reason: "reference", title };
    if (!valid(key, v)) return { kind: "blocked", reason: "invalid", title };
    inputs[key] = v;
  }
  return { kind: "ready", inputs };
}

const TTL = 60_000;
type Entry = { at: number; list: OptionList | null; error: OptionError | null; promise: Promise<OptionList> | null };
// (공급원, 입력) 묶음별 — 앱이 켜져 있는 동안 칸끼리 함께 쓴다.
const cache = new Map<string, Entry>();
const listeners = new Set<() => void>();

const cacheKey = (source: string, inputs: Record<string, string>) => source + JSON.stringify(Object.entries(inputs).sort());

function put(key: string, e: Entry) {
  cache.set(key, e);
  listeners.forEach((l) => l());
}

function subscribe(l: () => void) {
  listeners.add(l);
  return () => listeners.delete(l);
}

const toError = (e: unknown): OptionError =>
  typeof e === "object" && e !== null && "message" in e ? (e as OptionError) : { code: null, message: String(e) };

/** 새것이면 그대로, 같은 묶음을 부르는 중이면 그 요청을 함께 쓴다. `force`는 캐시를 무시한다.
 *  오류는 새것으로 보지 않는다 — 설정에서 연결하고 돌아와 다시 열면 바로 다시 부른다. */
function load(source: string, inputs: Record<string, string>, force: boolean) {
  const key = cacheKey(source, inputs);
  const e = cache.get(key);
  if (!force && e && (e.promise || (!e.error && Date.now() - e.at < TTL))) return;
  const promise = api.githubOptions(source, inputs);
  put(key, { at: e?.at ?? 0, list: e?.list ?? null, error: null, promise });
  // 다시 시도로 더 새 요청이 나갔으면 늦게 온 옛 결과는 버린다.
  const settle = (list: OptionList | null, error: OptionError | null) => {
    if (cache.get(key)?.promise === promise) put(key, { at: Date.now(), list, error, promise: null });
  };
  promise.then(
    (list) => settle(list, null),
    (err) => settle(null, toError(err)),
  );
}

const PREFETCH_DELAY = 300;

/** 칸이 화면에 나오면 미리 부르고(오류는 목록을 열 때만 보인다), 열 때 60초 지났으면 다시 부른다.
 *  미리 부르기는 입력이 300ms 멎은 뒤 — 앞 칸을 치는 동안 접두어마다 부르지 않게. 열 때는 바로. */
export function useGithubOptions(source: string, state: OptionsState, { open }: { open: boolean }) {
  const key = state.kind === "ready" ? cacheKey(source, state.inputs) : null;
  const entry = useSyncExternalStore(subscribe, () => (key ? cache.get(key) : undefined));
  useEffect(() => {
    if (state.kind !== "ready") return;
    const run = () => load(source, state.inputs, false);
    if (open) return run();
    const timer = setTimeout(run, PREFETCH_DELAY);
    return () => clearTimeout(timer);
    // key가 source·inputs를 담는다.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, open]);
  return {
    list: entry?.list ?? null,
    loading: !!entry?.promise,
    error: open ? (entry?.error ?? null) : null,
    retry: () => {
      if (state.kind === "ready") load(source, state.inputs, true);
    },
  };
}
