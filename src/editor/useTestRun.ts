import { useState } from "react";
import { api } from "../api";

/** 파일을 골라 한 번 실행 — 편집기 툴바와 실행 보기가 같이 쓴다. */
export function useTestRun(workflowId: string) {
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const run = async () => {
    setError(null);
    const path = await api.pickPath(false).catch((e) => (setError(String(e)), null));
    if (!path) return;
    setRunning(true);
    try {
      await api.runNow(workflowId, path);
    } catch (e) {
      setError(String(e));
    } finally {
      setRunning(false);
    }
  };
  return { running, error, run };
}
