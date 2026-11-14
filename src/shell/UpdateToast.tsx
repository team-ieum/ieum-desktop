import { useEffect, useState } from "react";
import { api, type UpdateProgress } from "../api";
import { t } from "../i18n";
import { BUTTON, PRIMARY } from "../screens/Editor";

// 이번 실행 동안의 알림 상태 — 설정 화면을 오가며 다시 마운트돼도 유지되도록 모듈에 둔다.
// [나중에] 누른 버전, 받는 중인 버전(두 번 누르지 못하게), 마지막 진행.
let dismissed: string | null = null;
let installing: string | null = null;
let lastProgress: UpdateProgress | null = null;

const MB = 1024 * 1024;
/** "3.2 / 6.6MB · 48%" — 전체를 모르면 "3.2MB". */
export const progressText = (p: UpdateProgress) =>
  p.total
    ? `${(p.downloaded / MB).toFixed(1)} / ${(p.total / MB).toFixed(1)}MB · ${Math.floor((p.downloaded * 100) / p.total)}%`
    : `${(p.downloaded / MB).toFixed(1)}MB`;

/** 새 버전 알림 — 상태 줄 바로 위 오른쪽 아래. [업데이트]는 받아 설치하고 앱을 다시 시작한다. */
export function UpdateToast() {
  const [version, setVersion] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState<UpdateProgress | null>(null);
  const [failed, setFailed] = useState(false);

  const show = (v: string) => {
    setVersion(v);
    // 받는 중이던 버전이면 그 상태를 이어서 보인다.
    setBusy(installing === v);
    setProgress(installing === v ? lastProgress : null);
  };

  useEffect(() => {
    const un = api
      .onUpdateAvailable((p) => {
        if (p.manual) dismissed = null;
        if (p.version !== dismissed) show(p.version);
      })
      .catch(() => () => {});
    const unProgress = api
      .onUpdateProgress((p) => {
        lastProgress = p;
        setProgress(p);
      })
      .catch(() => () => {});
    // 이벤트보다 늦게 뜬 화면도 Rust가 기억한 버전으로 띄운다.
    api.pendingUpdate().then((v) => v && v !== dismissed && show(v), () => {});
    return () => {
      void un.then((f) => f());
      void unProgress.then((f) => f());
    };
  }, []);

  const install = async () => {
    if (!version) return;
    installing = version;
    lastProgress = null;
    setBusy(true);
    setProgress(null);
    setFailed(false);
    try {
      await api.installUpdateAndRestart();
    } catch (e) {
      installing = null;
      setBusy(false);
      setProgress(null);
      // 그 사이 최신이 됐다(서버에서 내려감 등) — 알릴 것이 없으니 닫는다.
      if (e === "UP_TO_DATE") return setVersion(null);
      console.error("[ieum] 업데이트 설치 실패", e);
      setFailed(true);
    }
  };

  const pct = progress?.total ? Math.floor((progress.downloaded * 100) / progress.total) : null;
  // 상태 영역은 늘 두고 내용만 바꾼다(화면 낭독기가 나타난 알림을 읽게). 상태 줄(h-7) 위에 띄운다.
  return (
    <div role="status" aria-label={t("update.toast.label")} className="fixed bottom-9 right-4 z-40">
      {version && (
        <div className="w-80 rounded-xl border border-[#cde9f4] bg-white p-4 text-neutral-800 shadow-[0_20px_60px_-20px_rgba(41,83,124,.35)]">
          <div className="typo-body3_semibold text-main-deep-blue">{t("update.toast.title").replace("{version}", version)}</div>
          <p className={`mt-1 typo-caption1_medium ${failed ? "text-danger-700" : "text-neutral-700"}`}>
            {t(failed ? "update.toast.failed" : "update.toast.desc")}
          </p>
          {busy && progress && !progress.installing && (
            <div className="mt-3">
              <div
                role="progressbar"
                aria-label={t("update.toast.installing")}
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={pct ?? undefined}
                className="h-1.5 overflow-hidden rounded-full bg-main-light-blue"
              >
                {/* 전체를 모르면 막대를 채우지 않고 받은 크기만 글로 */}
                <div className="h-full rounded-full bg-main-blue transition-[width]" style={{ width: `${pct ?? 0}%` }} />
              </div>
              <p className="mt-1 text-right typo-caption2_medium tabular-nums text-neutral-500">{progressText(progress)}</p>
            </div>
          )}
          <div className="mt-3 flex justify-end gap-2">
            <button
              type="button"
              className={BUTTON}
              disabled={busy}
              onClick={() => {
                dismissed = version;
                setVersion(null);
              }}
            >
              {t("update.toast.later")}
            </button>
            <button type="button" className={PRIMARY} disabled={busy} onClick={() => void install()}>
              {t(!busy ? "update.toast.install" : progress?.installing ? "update.toast.applying" : "update.toast.installing")}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
