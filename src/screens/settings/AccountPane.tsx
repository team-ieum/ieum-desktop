import { LogIn, LogOut } from "lucide-react";
import { useState } from "react";
import { t } from "../../i18n";
import { useWeb, webErrorText } from "../../stores/web";
import { GHOST, Row } from "./Row";

export const serverHost = (url: string) => {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
};

/** 이메일·서버(표시만)·로그아웃 — 로그아웃하면 App이 관문으로 돌아간다. 만료면 [다시 로그인]이 관문을 연다. */
export function AccountPane() {
  const account = useWeb((s) => s.account);
  const logout = useWeb((s) => s.logout);
  const expired = useWeb((s) => s.expired);
  const setRelogin = useWeb((s) => s.setRelogin);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  if (!account) return null;

  const doLogout = async () => {
    setBusy(true);
    setError(null);
    try {
      await logout();
    } catch (e) {
      setError(webErrorText(e));
      setBusy(false);
    }
  };

  return (
    <>
      {expired && (
        <Row title={t("settings.sessionExpired")} desc={t("settings.reloginHint")} also={[t("settings.relogin")]}>
          <button type="button" className={GHOST} onClick={() => setRelogin(true)}>
            <LogIn className="size-[15px]" aria-hidden="true" />
            {t("settings.relogin")}
          </button>
        </Row>
      )}
      <Row title={account.email} desc={`${t("settings.loggedInAs")} · ${serverHost(account.serverUrl)}`} also={[t("settings.logout")]}>
        <button type="button" className={GHOST} disabled={busy} onClick={() => void doLogout()}>
          <LogOut className="size-[15px]" aria-hidden="true" />
          {t("settings.logout")}
        </button>
      </Row>
      {error && <p className="pb-3 typo-caption1_semibold text-danger-700">{error}</p>}
    </>
  );
}
