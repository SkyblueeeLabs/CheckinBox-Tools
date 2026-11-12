import type { CreditItem, Quota, QuotaPoint } from "../lib/types";
import { fmtDate, fmtDateTime, fmtPoints, fmtQuotaAmount, quotaLabels } from "../lib/api";
import { creditTypeLabel } from "../lib/credits";
import { expiryLabel, summarizeExpiry } from "../lib/expiry";
import CreditGroupsOverview from "./CreditGroupsOverview";
import TrendLine from "./TrendLine";

export interface QuotaCardProps {
  quota?: Quota | null;
  error?: string | null;
  loading?: boolean;
  onRefresh?: () => void;
  /** 平台 id：决定额度单位（Zcode 是 Token，其它平台是积分）与数字格式 */
  providerId?: string;
  /** 积分预警阈值 */
  alertThreshold?: number;
  /** 积分是否已低于阈值 */
  isLow?: boolean;
  /** 最近 7 天趋势 */
  trend?: QuotaPoint[];
  /** Qoder 每日领取的本地批次（平台逐包明细需网页登录，这里只记录本工具领过的） */
  trackedGrants?: CreditItem[];
  onGroup?: (kind: string) => void;
}

/**
 * 积分块：剩余（大字） + 已用/发放（等宽数字） + 用量条 + 批次明细。
 */
export default function QuotaCard({
  quota,
  error,
  loading,
  onRefresh,
  providerId,
  alertThreshold,
  isLow,
  trend,
  trackedGrants,
  onGroup,
}: QuotaCardProps) {
  const labels = quotaLabels(providerId);
  // Token 平台（Zcode）数值是 3 亿级，缩写展示；精确值放悬停提示
  const fmt = (n: number) => fmtQuotaAmount(n, providerId);
  // 悬停提示里的精确值：WorkBuddy 有小数，不能取整（否则和主数字对不上）
  const exact = (n: number) => (providerId === "workbuddy" ? fmt(n) : fmtPoints(n));

  const localGrants = trackedGrants && trackedGrants.length > 0 && (
    <details className="quota-details" open>
      <summary>每日领取记录（本地）· {trackedGrants.length} 批</summary>
      <p className="quota-note">这里记录通过 CheckinBox 领取的批次。实际扣减和余额以平台为准。</p>
      <table>
        <thead>
          <tr>
            <th>领取时间</th>
            <th className="r">{labels.unit}</th>
            <th>到期</th>
          </tr>
        </thead>
        <tbody>
          {trackedGrants.map((grant) => (
            <tr key={grant.grantedAtMs}>
              <td>{fmtDateTime(grant.grantedAtMs)}</td>
              <td className="r">{fmt(grant.granted)}</td>
              <td className={grant.expireAtMs < Date.now() ? "expired" : ""}>
                {expiryLabel(grant.expireAtMs)}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </details>
  );

  if (!quota || error) {
    return (
      <div className={`quota ${error ? "quota-error" : "quota-empty"}`}>
        <div className="quota-top">
          <div className="quota-remaining">
            <span className="q-label">{labels.remaining}</span>
            <span className="q-value" style={{ color: "var(--text-3)" }}>
              —
            </span>
          </div>
          {onRefresh && (
            <button className="btn small" onClick={onRefresh} disabled={loading}>
              {loading ? "正在刷新…" : error ? "重试刷新" : "刷新"}
            </button>
          )}
        </div>
        <p className="quota-msg" role={error ? "alert" : "status"}>
          {loading ? "正在读取额度…" : error || "暂无额度数据"}
        </p>
        {localGrants}
      </div>
    );
  }

  const { totalGranted, totalConsumed, totalRemaining, items, message, creditGroups } = quota;
  const usedPct = totalGranted > 0 ? (totalConsumed / totalGranted) * 100 : 0;
  const expiry = summarizeExpiry(items);
  const groups = creditGroups ?? [];
  const hasGroups = groups.length > 0;

  return (
    <div className="quota">
      <div className="quota-top">
        <div className="quota-remaining">
          <span className="q-label">{labels.remaining}</span>
          <span
            className="q-value"
            style={isLow ? { color: "var(--err)" } : undefined}
            title={`剩余 ${exact(totalRemaining)} ${labels.unit}`}
          >
            {fmt(totalRemaining)}
          </span>
        </div>

        <div className="quota-stats">
          <div className="qstat">
            <span className="q-label">已用</span>
            <span className="q-value">{fmt(totalConsumed)}</span>
            <span className="q-sub">{usedPct.toFixed(0)}%</span>
          </div>
          <div className="qstat">
            <span className="q-label">{labels.granted}</span>
            <span className="q-value">{fmt(totalGranted)}</span>
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

      {/* 有官方分组时到期/刷新线索跟着分组走，不再单独占一行 */}
      {!hasGroups && items.length > 0 && (
        <p className={`quota-expiry ${expiry.kind === "none" ? "dim" : ""}`}>
          {expiry.kind === "dated"
            ? `最近到期 ${expiryLabel(expiry.earliestMs)} · ${fmt(expiry.remaining)} ${labels.unit}`
            : expiry.kind === "long"
              ? "有效期：长期"
              : "到期时间：平台未提供"}
        </p>
      )}

      {quota.refreshAtMs ? (
        <p className="quota-expiry">下次刷新 {fmtDate(quota.refreshAtMs)}</p>
      ) : null}

      {isLow && (
        <p className="quota-alert">{labels.unit}低于预警线（{alertThreshold}）</p>
      )}

      <div className="quota-trend">
        <p className="credit-groups-head">近 7 天余额趋势</p>
        <TrendLine points={trend ?? []} threshold={alertThreshold} providerId={providerId} />
      </div>

      {hasGroups ? (
        <div className="credit-groups-section">
          <p className="credit-groups-head">积分分组</p>
          <CreditGroupsOverview groups={groups} items={items} onGroup={onGroup} />
        </div>
      ) : (
        items.length > 0 && (
          <details className="quota-details" open>
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
                    <td>{item.packageName || creditTypeLabel(item.creditType)}</td>
                    <td className="r" title={exact(item.granted)}>{fmt(item.granted)}</td>
                    <td className="r" title={exact(item.consumed)}>{fmt(item.consumed)}</td>
                    <td className="r" title={exact(item.remaining)}>{fmt(item.remaining)}</td>
                    <td>{expiryLabel(item.expireAtMs)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </details>
        )
      )}

      {localGrants}
    </div>
  );
}
