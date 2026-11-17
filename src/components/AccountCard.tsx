import { useId, useRef, useState } from "react";
import type { AccountView } from "../lib/types";
import { fmtPoints, fmtQuotaAmount, quotaLabels } from "../lib/api";
import { expiringCredits } from "../lib/expiry";
import { maskId, useMasked } from "../lib/mask";
import CreditsDialog from "./CreditsDialog";

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

/**
 * 状态徽章文案。
 *
 * `supportsClaim === false` 的平台（Zcode）不代领 —— 领取要过官方客户端的
 * 阿里云验证码，所以文案说「待领取 / 已领取」，按钮也是「打开客户端领取」。
 */
function stateText(supportsClaim: boolean, state: string): string {
  if (!supportsClaim) {
    switch (state) {
      case "claimable":
        return "待领取";
      case "claimed":
        return "已领取";
      case "notAvailable":
        return "暂无可领";
      default:
        return "状态未知";
    }
  }
  switch (state) {
    case "claimable":
      return "可签到";
    case "claimed":
      return "今日已签";
    case "notAvailable":
      return "暂无可签";
    default:
      return "状态未知";
  }
}

/** 小卡片上的一句话状态摘要 */
function summaryText(view: AccountView): string {
  const s = view.status;
  if (view.error) return "状态读取失败，点「重试」或重新导入";
  if (!s) return "正在读取…";
  const reminder = !view.supportsClaim;
  switch (s.state) {
    case "claimable":
      return reminder ? "待领取" : `可领 +${fmtPoints(s.points)}`;
    case "claimed":
      return reminder
        ? "已领取"
        : s.points > 0
          ? `已领 +${fmtPoints(s.points)}`
          : "今日已签";
    case "notAvailable":
      return reminder ? "暂无可领" : "今日无可签";
    default:
      return "状态未知";
  }
}

/**
 * 卡片展示账号状态、余额和常用操作，诊断信息可展开。
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
  const [creditsOpen, setCreditsOpen] = useState(false);
  const infoId = useId();
  const removeTitleId = useId();
  const removeDialog = useRef<HTMLDialogElement>(null);
  const { status, error, quotaError, account } = view;
  const state = status?.state ?? "unknown";
  const remaining = view.quota?.totalRemaining;
  // Zcode 的额度是 Token，其它平台是积分 —— 标签和数字格式都跟着单位走
  const unit = quotaLabels(account.provider);
  const detailsLabel = unit.unit === "Token" ? "额度明细" : "积分明细";
  const groups = view.quota?.creditGroups ?? [];
  const summary = summaryText(view);
  // 账号凭据失效这类错误会把卡片撑高 —— 收起态也要能一眼看出异常
  const hasError = !!error;
  const badge = hasError ? "读取失败" : stateText(view.supportsClaim, state);
  const canSwitchClient = switchable && !view.isClientLogin;
  const launchLabel = canSwitchClient
    ? "切换并打开客户端"
    : !view.supportsClaim && state !== "claimed"
      ? `打开${view.providerName}领取`
      : "打开客户端";
  const checkinLabel = hasError && state !== "claimable" ? "重试" : "签到";
  const refreshLabel = unit.unit === "Token" ? "刷新额度" : "刷新积分";
  // 积分和签到状态常报同一个错（同一个 401）：只显示一次，别把卡片撑高
  const shownQuotaError = quotaError && quotaError !== error ? quotaError : null;
  // 账号 id 副名字（与签到记录共用的打码开关）
  const masked = useMasked(account.provider);
  // 7 天内到期的积分（没有就是 null，不占位置）
  const expiring = expiringCredits(view.quota);
  // 周期面板（MiniMax 7 天）的逐天奖励，悬浮在「第 X/7 天」上
  const cycleTitle = status?.days?.length
    ? `本轮奖励：${status.days
        .map((d) => `第${d.dayNo}天 ${fmtPoints(d.points)}`)
        .join(" · ")}`
    : undefined;

  return (
    <article className={`acct-card ${state} ${view.isClientLogin ? "client-current" : ""} ${hasError ? "error" : ""} ${open ? "open" : ""}`}>
      <button
        className="acct-mini"
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
        aria-controls={open ? infoId : undefined}
        title={open ? "收起账号信息" : "展开账号信息"}
      >
        <span className="acct-mini-head">
          <span className={`acct-avatar ${view.isClientLogin ? "live" : ""}`}>
            {(account.label || "?").slice(0, 1)}
          </span>
          <span className="acct-mini-title">
            <strong title={account.label}>{account.label}</strong>
            <span className="acct-mini-sub" title={masked ? maskId(account.id) : account.id}>
              {masked ? maskId(account.id) : account.id}
            </span>
          </span>
          <span className={`acct-caret ${open ? "open" : ""}`} aria-hidden="true">▾</span>
        </span>

        <span className="acct-badges">
          {view.isClientLogin && <span className="chip-client">客户端登录</span>}
          {view.isActive && !view.isClientLogin && <span className="chip-active">当前账号</span>}
          {expiring && (
            <span
              className={`chip-expiry ${expiring.daysLeft <= 3 ? "urgent" : ""}`}
              title={`${fmtQuotaAmount(expiring.remaining, account.provider)} ${unit.unit}将在 ${expiring.daysLeft} 天内到期`}
            >
              {expiring.daysLeft} 天到期
            </span>
          )}
          {!view.isClientLogin && !view.isActive && !expiring && <span className="acct-saved">已保存账号</span>}
        </span>
      </button>

      <div className="acct-balance">
        <div className="acct-balance-head">
          <span className="acct-balance-label">{unit.remaining}</span>
          <button className="acct-credits-link" aria-haspopup="dialog" onClick={() => setCreditsOpen(true)}>
            {detailsLabel} <span aria-hidden="true">›</span>
          </button>
        </div>
        <div className="acct-mini-remaining">
          {remaining !== undefined && remaining !== null ? (
            <b
              className={`num ${view.isLow ? "low" : ""}`}
              title={`剩余 ${unit.unit === "Token" ? fmtPoints(remaining) : fmtQuotaAmount(remaining, account.provider)} ${unit.unit}`}
            >
              {fmtQuotaAmount(remaining, account.provider)}
            </b>
          ) : (
            <b className="num dim" title={quotaError ? "额度读取失败，可在明细中重试" : "暂无额度数据"}>—</b>
          )}
        </div>
        {groups.length > 0 ? (
          <dl className="acct-credit-groups">
            {groups.map((group) => (
              <div key={group.kind}>
                <dt>{group.name}</dt>
                <dd className="num">{fmtQuotaAmount(group.remaining, account.provider)}</dd>
                <dd className="acct-credit-track" aria-hidden="true" title={`剩余 ${fmtQuotaAmount(group.remaining, account.provider)} / ${fmtQuotaAmount(group.total, account.provider)}`}>
                  <span style={{ width: `${group.total > 0 ? Math.min(100, Math.max(0, group.remaining / group.total * 100)) : 0}%` }} />
                </dd>
              </div>
            ))}
          </dl>
        ) : (
          <dl className="acct-credit-totals">
            <div><dt>累计发放</dt><dd>{view.quota ? fmtQuotaAmount(view.quota.totalGranted, account.provider) : "—"}</dd></div>
            <div><dt>累计使用</dt><dd>{view.quota ? fmtQuotaAmount(view.quota.totalConsumed, account.provider) : "—"}</dd></div>
          </dl>
        )}
        <div className="acct-status-line">
          <span className={`acct-badge ${hasError ? "error" : state}`}>{badge}</span>
          {summary !== badge && <span className="acct-mini-summary" title={summary}>{summary}</span>}
        </div>
      </div>

      <div className="acct-footer" role="group" aria-label="账号操作">
        <button
          className="acct-launch"
          onClick={canSwitchClient ? onSwitchClient : onLaunch}
          disabled={busy}
          aria-label={launchLabel}
          title={canSwitchClient ? "备份原登录态，切换到此账号并重启客户端" : launchLabel}
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <path d="M13 5h6v6m0-6-9 9m7-1v5a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V8a1 1 0 0 1 1-1h5" />
          </svg>
          <span>{canSwitchClient ? "切换并打开" : launchLabel}</span>
        </button>
        <div className="acct-tools">
          <button
            className={`acct-tool acct-refresh ${quotaLoading ? "is-loading" : ""}`}
            onClick={onRefreshQuota}
            disabled={busy || quotaLoading}
            aria-label={refreshLabel}
            title={quotaLoading ? "正在刷新…" : `${refreshLabel}（立即拉取最新数据）`}
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <path d="M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8" />
              <path d="M21 3v5h-5" />
            </svg>
          </button>
          {view.supportsClaim && (
            <button
              className="acct-tool"
              onClick={onCheckin}
              disabled={busy || (state !== "claimable" && !hasError)}
              aria-label={checkinLabel}
              title={hasError ? "重新拉取状态并尝试签到" : "签到"}
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <rect x="4" y="5" width="16" height="15" rx="2" />
                <path d="M8 3v4m8-4v4M4 10h16m-11 5 2 2 4-4" />
              </svg>
            </button>
          )}
            <button className={`acct-tool ${view.isActive ? "is-selected" : ""}`} onClick={onSetActive} disabled={busy || view.isActive} aria-label={view.isActive ? "当前账号" : "设为当前"} title={view.isActive ? "当前账号" : "设为当前"}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <circle cx="12" cy="12" r="8" />
                <circle cx="12" cy="12" r="3" />
              </svg>
            </button>
          <button
            className="acct-tool acct-remove"
            aria-label="移除账号"
            title="移除账号"
            aria-haspopup="dialog"
            onClick={() => removeDialog.current?.showModal()}
            disabled={busy}
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <path d="M4 7h16m-2 0-1 13H7L6 7m3 0V4h6v3m-5 4v5m4-5v5" />
            </svg>
          </button>
        </div>
      </div>

      {open && (
        <div className="acct-detail" id={infoId}>
          <p className="acct-origin-line">{account.origin}</p>
          {shownQuotaError && <p className="acct-error">{shownQuotaError}</p>}

          <div>
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
              {status?.dayIndex ? <span title={cycleTitle}>第 {status.dayIndex}/7 天</span> : null}
              {status && status.streakDays > 0 ? (
                <span>连签 {status.streakDays} 天</span>
              ) : null}
              {status && status.bonusPoints > 0 ? (
                <span className="acct-bonus">额外 +{status.bonusPoints}</span>
              ) : null}
            </div>
          </div>

          <div className="acct-local">
            {view.projectCount} 项目 · {view.sessionCount} 会话
          </div>

          {view.dataDir && (
            <div className="acct-local">
              <span className="path">{view.dataDir}</span>
            </div>
          )}
        </div>
      )}
      <dialog className="confirm-dialog" ref={removeDialog} aria-labelledby={removeTitleId}>
        <div className="modal-card">
          <h2 id={removeTitleId}>移除账号？</h2>
          <p className="modal-text">
            将从 CheckinBox 移除 {view.providerName} 的账号「{account.label}」，并停止为它自动签到和同步。
          </p>
          <div className="modal-actions">
            <button className="btn" onClick={() => removeDialog.current?.close()}>
              取消
            </button>
            <button
              className="btn danger"
              disabled={busy}
              onClick={() => {
                removeDialog.current?.close();
                onRemove();
              }}
            >
              确认移除
            </button>
          </div>
        </div>
      </dialog>
      {creditsOpen && (
        <CreditsDialog
          accountId={account.id}
          accountLabel={account.label}
          providerId={account.provider}
          providerName={view.providerName}
          quota={view.quota}
          error={quotaError}
          loading={quotaLoading}
          onRefresh={onRefreshQuota}
          alertThreshold={view.alertThreshold}
          isLow={view.isLow}
          trend={view.trend}
          trackedGrants={view.trackedGrants}
          onClose={() => setCreditsOpen(false)}
        />
      )}
    </article>
  );
}
