import type { QuotaPoint } from "../lib/types";

interface Props {
  points: QuotaPoint[];
  /** 低于该值时用警示色 */
  threshold?: number;
  height?: number;
}

/**
 * 积分趋势迷你图（sparkline）。
 *
 * 数据点少时退化成几个方块，不做平滑 —— 保持「仪器面板」的克制感。
 * 只画折线 + 端点，不铺面积、不加网格。
 */
export default function TrendLine({ points, threshold, height = 30 }: Props) {
  if (points.length < 2) {
    return <div className="trend trend-empty">趋势数据收集中…</div>;
  }

  const values = points.map((p) => p.remaining);
  const max = Math.max(...values, threshold ?? 0);
  const min = Math.min(...values);
  const span = max - min || 1;

  const w = 120;
  const x = (i: number) => (i / (points.length - 1)) * (w - 4) + 2;
  const y = (v: number) => height - 3 - ((v - min) / span) * (height - 6);

  const path = points
    .map((p, i) => `${i === 0 ? "M" : "L"}${x(i).toFixed(1)},${y(p.remaining).toFixed(1)}`)
    .join(" ");

  const last = points[points.length - 1];
  const first = points[0];
  const delta = last.remaining - first.remaining;

  const danger = threshold !== undefined && last.remaining < threshold;

  return (
    <div className="trend">
      <svg width={w} height={height} className={danger ? "trend-svg danger" : "trend-svg"}>
        {threshold !== undefined && max > threshold && (
          <line
            x1={0}
            x2={w}
            y1={y(threshold).toFixed(1)}
            y2={y(threshold).toFixed(1)}
            className="trend-threshold"
          />
        )}
        <path d={path} className="trend-path" />
        <circle cx={x(points.length - 1).toFixed(1)} cy={y(last.remaining).toFixed(1)} r={2.2} />
        {/* 悬浮数值：每个点一个透明命中区，title 显示时间和剩余积分 */}
        {points.map((p, i) => (
          <circle key={i} className="trend-hit" cx={x(i)} cy={y(p.remaining)} r={5}>
            <title>{`${p.timestamp.slice(5)} · 剩余 ${Math.round(p.remaining)}`}</title>
          </circle>
        ))}
      </svg>

      <div className="trend-meta">
        <span className={delta < 0 ? "trend-delta down" : "trend-delta"}>
          {delta > 0 ? "+" : ""}
          {Math.round(delta)}
        </span>
        <span className="trend-range">
          {points.length} 次 · {Math.round(min)}–{Math.round(max)}
        </span>
      </div>
    </div>
  );
}
