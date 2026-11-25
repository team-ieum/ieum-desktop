// @vitest-environment jsdom
import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CLOSE_MS, useClosing } from "./useClosing";

const w = window as unknown as { matchMedia?: unknown };

describe("useClosing", () => {
	beforeEach(() => vi.useFakeTimers());
	afterEach(() => {
		vi.useRealTimers();
		delete w.matchMedia;
	});

	it("closes_at_once_without_motion", () => {
		const onClose = vi.fn();
		const { result } = renderHook(() => useClosing(onClose));
		act(() => result.current.close());
		expect(onClose).toHaveBeenCalledTimes(1);
		expect(result.current.closing).toBe(false);
	});

	it("waits_160ms_with_motion", () => {
		w.matchMedia = () => ({ matches: true });
		const onClose = vi.fn();
		const { result } = renderHook(() => useClosing(onClose));
		act(() => result.current.close());
		expect(result.current.closing).toBe(true);
		act(() => void vi.advanceTimersByTime(CLOSE_MS - 1));
		expect(onClose).toHaveBeenCalledTimes(0);
		act(() => void vi.advanceTimersByTime(1));
		expect(onClose).toHaveBeenCalledTimes(1);
		expect(result.current.closing).toBe(false); // 닫힘이 끝나면 다음 열기를 위해 되돌린다
	});

	it("custom_delay", () => {
		w.matchMedia = () => ({ matches: true });
		const onClose = vi.fn();
		const { result } = renderHook(() => useClosing(onClose, 240));
		act(() => result.current.close());
		act(() => void vi.advanceTimersByTime(239));
		expect(onClose).toHaveBeenCalledTimes(0);
		act(() => void vi.advanceTimersByTime(1));
		expect(onClose).toHaveBeenCalledTimes(1);
	});

	it("reopen_cancels_pending_close", () => {
		w.matchMedia = () => ({ matches: true });
		const onClose = vi.fn();
		const { result } = renderHook(() => useClosing(onClose));
		act(() => result.current.close());
		act(() => result.current.cancel());
		act(() => void vi.advanceTimersByTime(200));
		expect(onClose).toHaveBeenCalledTimes(0);
		expect(result.current.closing).toBe(false);
	});

	it("unmount_clears_timer", () => {
		w.matchMedia = () => ({ matches: true });
		const onClose = vi.fn();
		const { result, unmount } = renderHook(() => useClosing(onClose));
		act(() => result.current.close());
		unmount();
		act(() => void vi.advanceTimersByTime(200));
		expect(onClose).toHaveBeenCalledTimes(0);
	});
});
