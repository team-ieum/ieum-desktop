import { useEffect, useRef } from "react";

/**
 * 떠 있는 동안 Esc를 document capture 단계에서 먼저 받는다 — 포커스가 어디 있든(body로 빠져도)
 * 뒤의 SidePanel(document `useEscape`)까지 가지 않게. 한글 조합 중이거나 다른 모달(⌘K 검색 등) 안의 Esc는 그쪽 몫이라 건드리지 않는다.
 */
export function useCaptureEscape(onEscape: () => void) {
  const latest = useRef(onEscape);
  latest.current = onEscape;
  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.isComposing) return;
      const el = e.target instanceof Element ? e.target : null;
      if (el?.closest('[aria-modal="true"]')) return;
      e.stopPropagation();
      latest.current();
    };
    document.addEventListener("keydown", h, true);
    return () => document.removeEventListener("keydown", h, true);
  }, []);
}
