import { useCallback, useEffect, useRef, useState } from "react";

/** 닫는 움직임 길이 — index.css의 ieum-slide-out(0.16초)과 맞춘다. */
export const CLOSE_MS = 160;

/**
 * 닫기 전에 퇴장 움직임을 보여 주는 훅. 움직임 허용일 때만 `closing`을 켜고 CLOSE_MS 뒤 끄면서 onClose,
 * 동작 줄이기·jsdom(matchMedia 없음)이면 바로 onClose.
 */
export function useClosing(onClose: () => void, ms: number = CLOSE_MS) {
	const [closing, setClosing] = useState(false);
	const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
	const latest = useRef(onClose);
	latest.current = onClose;

	const cancel = useCallback(() => {
		clearTimeout(timer.current);
		timer.current = undefined;
		setClosing(false);
	}, []);
	const close = useCallback(() => {
		if (!window.matchMedia?.("(prefers-reduced-motion: no-preference)").matches) return latest.current();
		setClosing(true);
		clearTimeout(timer.current);
		timer.current = setTimeout(() => {
			setClosing(false);
			latest.current();
		}, ms);
	}, [ms]);
	useEffect(() => () => clearTimeout(timer.current), []);

	return { closing, close, cancel };
}
