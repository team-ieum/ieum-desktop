import { useId } from "react";

const W = 736;
const L = 30;
const R = 726;
const TOP = 8;
const BASE = 108;

/** 시간대별 실행 면 그래프 — SVG 직접. 색은 감싸는 쪽 `currentColor`. */
export function AreaChart({ series, height = 130 }: { series: { t: string; count: number }[]; height?: number }) {
  const gid = useId();
  // y 눈금은 정수 5줄 — 꼭대기를 4의 배수로.
  const top = Math.max(4, Math.ceil(Math.max(0, ...series.map((s) => s.count)) / 4) * 4);
  const x = (i: number) => L + ((R - L) * i) / Math.max(1, series.length - 1);
  const y = (v: number) => BASE - ((BASE - TOP) * v) / top;
  const pts = series.map((s, i) => [x(i), y(s.count)] as const);
  // 가로 가운데를 조절점으로 쓰는 큐빅 — 점 사이에서 위아래로 넘치지 않는다.
  const line = pts
    .map(([px, py], i) => {
      if (i === 0) return `M${px},${py}`;
      const [qx, qy] = pts[i - 1];
      const mx = (qx + px) / 2;
      return `C${mx},${qy} ${mx},${py} ${px},${py}`;
    })
    .join(" ");
  const ticks = [0, 1, 2, 3, 4].map((k) => (top * k) / 4);

  return (
    <svg viewBox={`0 0 ${W} 130`} height={height} className="block w-full" fontSize="11" fill="currentColor" role="img">
      <defs>
        <linearGradient id={gid} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="currentColor" stopOpacity=".7" />
          <stop offset="1" stopColor="currentColor" stopOpacity="0" />
        </linearGradient>
      </defs>
      <g stroke="#007ba7" strokeDasharray="2 4" strokeWidth="1">
        {ticks.map((v) => (
          <line key={v} x1={L} x2={R} y1={y(v)} y2={y(v)} />
        ))}
      </g>
      <g fillOpacity=".85" textAnchor="end">
        {ticks.map((v) => (
          <text key={v} x="18" y={y(v) + 4}>
            {v}
          </text>
        ))}
      </g>
      {pts.length > 1 && (
        <>
          <path fill={`url(#${gid})`} d={`${line} L${R},${BASE} L${L},${BASE} Z`} />
          <path fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" d={line} />
        </>
      )}
      <g fillOpacity=".85" textAnchor="middle">
        {series.map((s, i) => (
          <text key={s.t} x={x(i)} y="126">
            {s.t}
          </text>
        ))}
      </g>
    </svg>
  );
}
