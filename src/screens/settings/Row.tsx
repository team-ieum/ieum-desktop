import { createContext, useContext, type ReactNode } from "react";

/** 설정 검색어 — 이름·설명에 맞지 않는 줄은 숨는다. */
export const QueryContext = createContext("");

export const matches = (q: string, ...texts: string[]) => {
  const n = q.trim().toLowerCase();
  return !n || texts.some((s) => s.toLowerCase().includes(n));
};

export const GHOST =
  "inline-flex h-9 shrink-0 items-center gap-1.5 rounded-[10px] border border-neutral-200 bg-white px-4 text-sm font-semibold text-neutral-700 hover:bg-neutral-50 disabled:opacity-40";

/** 왼쪽 이름·설명, 오른쪽 조작. `below`는 줄 아래로 펼치는 내용(키 입력 등), `also`는 설명·버튼처럼 검색에 걸릴 다른 문구. */
export function Row({
  title,
  desc,
  also = [],
  children,
  below,
}: {
  title: string;
  desc?: ReactNode;
  also?: string[];
  children?: ReactNode;
  below?: ReactNode;
}) {
  const q = useContext(QueryContext);
  if (!matches(q, title, typeof desc === "string" ? desc : "", ...also)) return null;
  return (
    <div className="border-b border-neutral-100 py-3.5 last:border-0">
      <div className="flex items-center justify-between gap-6">
        <div className="min-w-0">
          <div className="typo-body2_semibold text-neutral-800">{title}</div>
          {desc && <div className="mt-0.5 typo-caption1_regular text-neutral-500">{desc}</div>}
        </div>
        {children}
      </div>
      {below && <div className="mt-3">{below}</div>}
    </div>
  );
}
