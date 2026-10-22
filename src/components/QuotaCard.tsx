import type { Quota } from "../lib/types";
import { fmtDate, fmtPoints } from "../lib/api";

interface Props {
  quota?: Quota | null;
  error?: string | null;
  loading?: boolean;
  onRefresh?: () => void;
}

/**
 * 积分块：剩余（大字） + 已用/发放（等宽数字） + 用量条 + 批次明细。
 */
export default function QuotaCard({ quota, error, loading, onRefresh }: Props) {
  if (error) {
    return (
      <div className="quota quota-error">
        <div className="quota-top">
          <div className="quota-remaining">
            <span className="q-label">Credits</span>
            <span className="q-value">—</span>
          </div>
          {onRefresh && (
            <button className="link" onClick={onRefresh}>
              重试
            </button>
          )}
        </div>
        <p className="quota-msg">{error}</p>
      </div>
    );
  }

  if (!quota) {
    return (
      <div className="quota quota-empty">
        <div className="quota-top">
          <div className="quota-remaining">
            <span className="q-label">Credits</span>
            <span className="q-value" style={{ color: "var(--text-3)" }}>
              —
            </span>
          </div>
        </div>
        <p className="quota-msg">{loading ? "读取中…" : "暂无数据"}</p>
      </div>
    );
  }

  const { totalGranted, totalConsumed, totalRemaining, items, message } = quota;
  const usedPct = totalGranted > 0 ? (totalConsumed / totalGranted) * 100 : 0;

  return (
    <div className="quota">
      <div className="quota-top">
        <div className="quota-remaining">
          <span className="q-label">剩余</span>
          <span className="q-value">{fmtPoints(totalRemaining)}</span>
        </div>

        <div className="quota-stats">
          <div className="qstat">
            <span className="q-label">已用</span>
            <span className="q-value">{fmtPoints(totalConsumed)}</span>
            <span className="q-sub">{usedPct.toFixed(0)}%</span>
          </div>
          <div className="qstat">
            <span className="q-label">发放</span>
            <span className="q-value">{fmtPoints(totalGranted)}</span>
          </div>
          {onRefresh && (
            <button className="link" onClick={onRefresh} disabled={loading}>
              {loading ? "…" : "刷新"}
            </button>
          )}
        </div>
      </div>

      <div className="quota-bar">
        <span style={{ width: `${Math.min(100, usedPct)}%` }} />
      </div>

      {message && <p className="quota-msg">{message}</p>}

      {items.length > 0 && (
        <details className="quota-details">
          <summary>{items.length} 个发放批次</summary>
          <table>
            <thead>
              <tr>
                <th>类别</th>
                <th className="r">发放</th>
                <th className="r">已用</th>
                <th className="r">剩余</th>
                <th>到期</th>
              </tr>
            </thead>
            <tbody>
              {items.map((item, i) => (
                <tr key={i}>
                  <td>{item.creditType === "purchased" ? "购买" : item.creditType === "plan" ? "套餐" : item.creditType === "addon" ? "加购" : "签到"}</td>
                  <td className="r">{fmtPoints(item.granted)}</td>
                  <td className="r">{fmtPoints(item.consumed)}</td>
                  <td className="r">{fmtPoints(item.remaining)}</td>
                  <td>{fmtDate(item.expireAtMs)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </details>
      )}
    </div>
  );
}
