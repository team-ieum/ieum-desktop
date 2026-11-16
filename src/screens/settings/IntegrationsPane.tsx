import { useCallback, useEffect, useRef, useState } from "react";
import { api, type GithubStatus } from "../../api";
import { t, type Key } from "../../i18n";
import { GHOST } from "./Row";

const PRIMARY = "inline-flex h-9 shrink-0 items-center rounded-[10px] bg-main-deep-blue px-4 text-sm font-semibold text-white hover:opacity-90 disabled:opacity-40";
const OK = "bg-node-green/10 text-node-green";
const WARN = "bg-amber-100 text-amber-800";
const IDLE = "bg-neutral-100 text-neutral-600";
const BADGE: Record<GithubStatus["kind"], [Key, string]> = {
  notInstalled: ["settings.gh.notInstalled", IDLE],
  outdated: ["settings.gh.updateNeeded", WARN],
  notConnected: ["settings.gh.notConnected", IDLE],
  needsLogin: ["settings.loginNeeded", WARN],
  accountChanged: ["settings.gh.changed", WARN],
  connected: ["settings.connected", OK],
};

/** 설정 검색용 — 이 페이지에 나올 수 있는 문구. */
export function integrationsTexts(): string[] {
  return [
    "GitHub",
    t("settings.gh.desc"),
    t("settings.gh.missing"),
    t("settings.gh.install"),
    t("settings.gh.update"),
    t("settings.gh.connect"),
    t("settings.disconnect"),
    t("settings.gh.needsLogin"),
    t("settings.gh.envToken"),
  ];
}

function GithubMark() {
  return (
    <svg viewBox="0 0 16 16" className="size-9 shrink-0 text-neutral-900" fill="currentColor" aria-hidden="true">
      <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z" />
    </svg>
  );
}

type Job = "install" | "connect";

/** 설정 > 연동 — GitHub 줄 하나. 상태는 열 때·[다시 확인]·작업 끝에 다시 읽는다. */
export function IntegrationsPane() {
  const [status, setStatus] = useState<GithubStatus | null>(null);
  const [job, setJob] = useState<Job | null>(null);
  const [code, setCode] = useState<string | null>(null);
  const [lines, setLines] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const busy = useRef(false);
  /** [다시 시도]가 다시 할 일 — 없으면 상태 다시 읽기. */
  const last = useRef<(() => void) | null>(null);

  const load = useCallback(() => {
    api.githubStatus().then(setStatus, (e) => setError(String(e)));
  }, []);

  useEffect(() => {
    load();
    const subs = [
      api.onGithubLoginCode((p) => setCode(p.code)),
      api.onGithubInstallProgress((p) => setLines((l) => [...l, p.line].slice(-20))),
    ];
    return () => {
      subs.forEach((s) => s.then((off) => off()));
      // 로그인·설치 중에 화면을 떠나면 gh 프로세스를 멈춘다.
      if (busy.current) api.githubCancel().catch(() => {});
    };
  }, [load]);

  const run = (kind: Job, call: () => Promise<GithubStatus>) => {
    const go = () => {
      busy.current = true;
      setJob(kind);
      setError(null);
      setCode(null);
      setLines([]);
      call()
        .then(setStatus, (e) => {
          setError(String(e));
          load();
        })
        .finally(() => {
          busy.current = false;
          setJob(null);
          setCode(null);
        });
    };
    last.current = go;
    go();
  };
  const install = () => run("install", api.githubInstall);
  const connect = () => run("connect", api.githubConnect);
  const disconnect = () => {
    last.current = disconnect;
    setError(null);
    api.githubDisconnect().then(setStatus, (e) => setError(String(e)));
  };
  const cancel = () => void api.githubCancel().catch((e) => setError(String(e)));
  const openDevice = () => void api.githubOpenDevice().catch((e) => setError(String(e)));

  const recheck = (
    <button type="button" className={GHOST} onClick={load}>
      {t("settings.gh.recheck")}
    </button>
  );
  const s = status;
  let note: string | null = null;
  let actions = null;
  if (job) {
    actions = (
      <button type="button" className={GHOST} onClick={cancel}>
        {t("common.cancel")}
      </button>
    );
  } else if (s?.kind === "notInstalled") {
    note = t("settings.gh.missing");
    actions = (
      <>
        <button type="button" className={PRIMARY} onClick={install}>
          {t("settings.gh.install")}
        </button>
        {recheck}
      </>
    );
  } else if (s?.kind === "outdated") {
    note = t("settings.gh.outdated").replace("{version}", s.version);
    actions = (
      <>
        <button type="button" className={PRIMARY} onClick={install}>
          {t("settings.gh.update")}
        </button>
        {recheck}
      </>
    );
  } else if (s?.kind === "notConnected" || s?.kind === "needsLogin") {
    if (s.kind === "needsLogin") note = t("settings.gh.needsLogin");
    actions = (
      <button type="button" className={PRIMARY} onClick={connect}>
        {t("settings.gh.connect")}
      </button>
    );
  } else if (s?.kind === "accountChanged") {
    note = t("settings.gh.accountChanged").replace("{current}", s.current);
    actions = (
      <>
        <button type="button" className={PRIMARY} onClick={connect}>
          {t("settings.gh.useAccount")}
        </button>
        <button type="button" className={GHOST} onClick={disconnect}>
          {t("settings.disconnect")}
        </button>
      </>
    );
  } else if (s?.kind === "connected") {
    actions = (
      <button type="button" className={GHOST} onClick={disconnect}>
        {t("settings.disconnect")}
      </button>
    );
  }
  const [badgeKey, badgeClass] = s ? BADGE[s.kind] : (["common.loading", IDLE] as [Key, string]);

  return (
    <div className="py-3.5">
      <div className="flex items-center gap-3">
        <GithubMark />
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <span className="typo-body2_semibold text-neutral-800">GitHub</span>
            {s?.kind === "connected" && <span className="truncate typo-body3_regular text-neutral-600">{s.login}</span>}
            {!job && <span className={`shrink-0 rounded-full px-2.5 py-0.5 typo-caption1_semibold ${badgeClass}`}>{t(badgeKey)}</span>}
          </div>
          <div className="mt-0.5 typo-caption1_regular text-neutral-500">{t("settings.gh.desc")}</div>
        </div>
        <div className="flex shrink-0 items-center gap-2">{actions}</div>
      </div>

      <div className="pl-12">
        {job === "install" && <p className="mt-2 typo-caption1_regular text-neutral-600">{t("settings.gh.installing")}</p>}
        {job === "connect" && !code && <p className="mt-2 typo-caption1_regular text-neutral-600">{t("settings.gh.connecting")}</p>}
        {job === "connect" && code && (
          <div className="mt-3 rounded-[10px] border border-[#cde9f4] bg-main-light-blue p-4">
            <div className="flex items-center gap-3">
              <span className="typo-caption1_semibold text-main-gray">{t("settings.gh.code")}</span>
              <span className="flex-1 font-mono text-lg font-semibold tracking-[.12em] text-main-deep-blue">{code}</span>
              <button type="button" className={GHOST} onClick={() => void navigator.clipboard.writeText(code).catch((e) => setError(String(e)))}>
                {t("settings.gh.copy")}
              </button>
              <button type="button" className={PRIMARY} onClick={openDevice}>
                {t("settings.gh.openBrowser")}
              </button>
            </div>
            <p className="mt-2 typo-caption1_regular text-neutral-600">{t("settings.gh.codeHint")}</p>
          </div>
        )}
        {!job && note && <p className="mt-2 typo-caption1_regular text-neutral-600">{note}</p>}
        {!job && s?.kind === "connected" && s.envToken && <p className="mt-2 typo-caption1_regular text-neutral-600">{t("settings.gh.envToken")}</p>}
        {error && (
          <div className="mt-2 flex items-center gap-3">
            <p className="min-w-0 flex-1 typo-caption1_semibold text-danger-700">{error}</p>
            {!job && (
              <button type="button" className={GHOST} onClick={() => (last.current ?? load)()}>
                {t("settings.gh.retry")}
              </button>
            )}
          </div>
        )}
        {lines.length > 0 && (
          <details className="mt-2">
            <summary className="cursor-pointer typo-caption1_semibold text-neutral-500">{t("settings.gh.log")}</summary>
            <pre className="mt-1 max-h-60 overflow-auto rounded-lg bg-neutral-50 p-3 font-mono text-[11px] text-neutral-700">{lines.join("\n")}</pre>
          </details>
        )}
      </div>
    </div>
  );
}
