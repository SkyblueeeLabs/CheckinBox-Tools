import { useState } from "react";
import type { AccountView } from "../lib/types";
import { fmtPoints } from "../lib/api";
import { maskId, useMasked } from "../lib/mask";
import QuotaCard from "./QuotaCard";

interface Props {
  view: AccountView;
  busy: boolean;
  quotaLoading: boolean;
  /** 这个平台的客户端能不能靠写凭据文件换账号 */
  switchable: boolean;
  onSwitchClient: () => void;
  onCheckin: () => void;
  onRefreshQuota: () => void;
  onSetActive: () => void;
  onLaunch: () => void;
  onRemove: () => void;
}

const STATE_TEXT: Record<string, string> = {
  claimable: "可签到",
  claimed: "今日已签",
  notAvailable: "暂无可签",
  unknown: "状态未知",
};

/** 小卡片上的一句话状态摘要 */
function summaryText(view: AccountView): string {
  const s = view.status;
  if (view.error) return "状态读取失败，点「重试」或重新导入";
  if (!s) return "正在读取…";
  switch (s.state) {
    case "claimable":
      return `可领 +${fmtPoints(s.points)}`;
    case "claimed":
      return s.points > 0 ? `已领 +${fmtPoints(s.points)}` : "今日已签";
    case "notAvailable":
      return "今日无可签";
    default:
      return "状态未知";
  }
}

/**
 * 账号小卡片：收起时只展示关键信息（谁、状态、剩余积分），
 * 点击展开完整详情（积分明细 / 趋势 / 签到 / 操作）。
 * 多账号时小卡片排成网格，展开的那张占满一行。
 */
export default function AccountCard({
  view,
  busy,
  quotaLoading,
  switchable,
  onSwitchClient,
  onCheckin,
  onRefreshQuota,
  onSetActive,
  onLaunch,
  onRemove,
}: Props) {
  const [open, setOpen] = useState(false);
  const { status, error, quotaError, account } = view;
  const state = status?.state ?? "unknown";
  const remaining = view.quota?.totalRemaining;
  // 账号凭据失效这类错误会把卡片撑高 —— 收起态也要能一眼看出异常
  const hasError = !!error;
  // 积分和签到状态常报同一个错（同一个 401）：只显示一次，别把卡片撑高
  const shownQuotaError = quotaError && quotaError !== error ? quotaError : null;
  // 账号 id 副名字（与签到记录共用的打码开关）
  const masked = useMasked(account.provider);

  return (
    <article className={`acct-card ${state} ${hasError ? "error" : ""} ${open ? "open" : ""}`}>
      {/* ---------- 收起态：关键信息 ---------- */}
      <button
        className="acct-mini"
        onClick={() => setOpen((v) => !v)}
        title={open ? "收起详情" : "展开详情"}
      >
        <span className="acct-mini-head">
          <span className={`acct-avatar ${view.isClientLogin ? "live" : ""}`}>
            {(account.label || "?").slice(0, 1)}
          </span>
          <span className="acct-mini-title">
            <strong>{account.label}</strong>
            <span className="acct-mini-sub" title={account.id}>
              {masked ? maskId(account.id) : account.id}
            </span>
            <span className="acct-badges">
              {view.isClientLogin && <span className="chip-client">客户端登录</span>}
              {view.isActive && <span className="chip-active">当前账号</span>}
              <span className={`acct-badge ${hasError ? "error" : state}`}>
                {hasError ? "读取失败" : STATE_TEXT[state] ?? state}
              </span>
            </span>
          </span>
        </span>

        <span className="acct-mini-stats">
          <span className="acct-mini-remaining">
            {remaining !== undefined && remaining !== null ? (
              <b className={`num ${view.isLow ? "low" : ""}`}>{fmtPoints(remaining)}</b>
            ) : (
              <b className="num dim">—</b>
            )}
            <i>剩余总积分</i>
          </span>
          <span className="acct-mini-summary">{summaryText(view)}</span>
          <span className={`acct-caret ${open ? "open" : ""}`}>▾</span>
        </span>
      </button>

      {/* ---------- 展开态：完整详情 ---------- */}
      {open && (
        <div className="acct-detail">
          <p className="acct-origin-line">{account.origin}</p>

          <QuotaCard
            quota={view.quota}
            error={shownQuotaError}
            loading={quotaLoading}
            onRefresh={onRefreshQuota}
            alertThreshold={view.alertThreshold}
            isLow={view.isLow}
            trend={view.trend}
          />

          <div className="acct-signin">
            <div style={{ minWidth: 0 }}>
              {error ? (
                <p className="acct-error" title={error}>
                  {error}
                </p>
              ) : status ? (
                <p className="acct-status-msg">{status.message}</p>
              ) : (
                <p className="acct-status-msg dim">正在读取签到状态…</p>
              )}

              <div className="acct-meta">
                {status?.dayIndex ? <span>第 {status.dayIndex}/7 天</span> : null}
                {status && status.streakDays > 0 ? (
                  <span>连签 {status.streakDays} 天</span>
                ) : null}
                {status && status.bonusPoints > 0 ? (
                  <span className="acct-bonus">额外 +{status.bonusPoints}</span>
                ) : null}
              </div>
            </div>

            <button
              className="btn small primary"
              onClick={onCheckin}
              disabled={busy || (state !== "claimable" && !hasError)}
              title={hasError ? "重新拉取状态并尝试签到" : undefined}
            >
              {state === "claimable" ? "签到" : hasError ? "重试" : "已处理"}
            </button>
          </div>

          <div className="acct-local">
            <span>
              {view.projectCount} 项目 · {view.sessionCount} 会话
            </span>
            <div style={{ display: "flex", gap: 10 }}>
              {!view.isActive && (
                <button className="link" onClick={onSetActive} disabled={busy}>
                  设为当前
                </button>
              )}
              {view.isClientLogin || !switchable ? (
                /* 客户端当前登的就是这个账号：直接打开 */
                <button className="link" onClick={onLaunch} disabled={busy}>
                  打开客户端
                </button>
              ) : (
                /* 换号一条龙：自动关闭客户端 → 写入此账号凭据 → 以它重启 */
                <button
                  className="link"
                  onClick={onSwitchClient}
                  disabled={busy}
                  title="自动关闭客户端 → 备份登录态 → 写入这个账号的凭据 → 以它重启（借鉴 Cockpit）"
                >
                  切换并打开客户端
                </button>
              )}
              <button className="link" onClick={onRemove} disabled={busy}>
                移除
              </button>
            </div>
          </div>

          {view.dataDir && (
            <div className="acct-local">
              <span className="path">{view.dataDir}</span>
            </div>
          )}
        </div>
      )}
    </article>
  );
}
