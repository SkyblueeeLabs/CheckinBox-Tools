import type { Quota } from "../lib/types";
import { fmtDate, fmtPoints } from "../lib/api";

interface Props {
  quota?: Quota | null;
  error?: string | null;
  loading?: boolean;
  onRefresh?: () => void;
}

/**
 * 积分卡：发放 / 已用 / 剩余。
 *
 * 「剩余」用大字显示（用户最关心的），「已用」和「发放」作为辅助行，
 * 明细批次折叠在下方，避免一屏塞满。
 */
export default function QuotaCard({ quota, error, loading, onRefresh }: Props) {
  if (error) {
    return (
      <div className="quota quota-error">
        <span className="quota-label">积分</span>
        <span className="quota-msg">{error}</span>
        {onRefresh && (
          <button className="link" onClick={onRefresh}>
            重试
          </button>
        )}
      </div>
    );
  }

  if (!quota) {
    return (
      <div className="quota quota-empty">
        <span className="quota-label">积分</span>
        <span className="quota-msg">{loading ? "读取中…" : "暂无数据"}</span>
      </div>
    );
  }

  const { totalGranted, totalConsumed, totalRemaining, items, message } = quota;
  const usedPct = totalGranted > 0 ? (totalConsumed / totalGranted) * 100 : 0;

  return (
    <div className="quota">
      <div className="quota-main">
        <div className="quota-remaining">
          <span className="quota-num">{fmtPoints(totalRemaining)}</span>
          <span className="quota-unit">剩余</span>
        </div>

        <div className="quota-bars">
          <div className="quota-row">
            <span className="quota-key">已用</span>
            <span className="quota-val">{fmtPoints(totalConsumed)}</span>
            <span className="quota-pct">{usedPct.toFixed(0)}%</span>
          </div>
          <div className="quota-track">
            <span
              className="quota-fill used"
              style={{ width: `${Math.min(100, usedPct)}%` }}
            />
          </div>
          <div className="quota-row">
            <span className="quota-key">发放</span>
            <span className="quota-val">{fmtPoints(totalGranted)}</span>
          </div>
        </div>

        {onRefresh && (
          <button className="link quota-refresh" onClick={onRefresh} disabled={loading}>
            {loading ? "刷新中" : "刷新"}
          </button>
        )}
      </div>

      {message && <p className="quota-msg">{message}</p>}

      {items.length > 0 && (
        <details className="quota-details">
          <summary>{items.length} 个发放批次</summary>
          <table>
            <thead>
              <tr>
                <th>类别</th>
                <th className="num">发放</th>
                <th className="num">已用</th>
                <th className="num">剩余</th>
                <th>到期</th>
              </tr>
            </thead>
            <tbody>
              {items.map((item, i) => (
                <tr key={i}>
                  <td>
                    {item.creditType === "purchased" ? "购买" : "签到"}
                  </td>
                  <td className="num">{fmtPoints(item.granted)}</td>
                  <td className="num">{fmtPoints(item.consumed)}</td>
                  <td className="num">{fmtPoints(item.remaining)}</td>
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
