import type { AccountView } from "../lib/types";
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
  const { status, error, account } = view;
  const state = status?.state ?? "unknown";

  return (
    <article className={`acct ${state}`}>
      <header className="acct-head">
        <div className="acct-who">
          <span className="acct-avatar">{(account.label || "?").slice(0, 1)}</span>
          <div>
            <strong>{account.label}</strong>
            <span className="acct-origin">{account.origin}</span>
          </div>
        </div>

        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          {view.isActive && <span className="chip-active">当前账号</span>}
          <span className={`acct-badge ${state}`}>{STATE_TEXT[state] ?? state}</span>
        </div>
      </header>

      <QuotaCard
        quota={view.quota}
        error={view.quotaError}
        loading={quotaLoading}
        onRefresh={onRefreshQuota}
        alertThreshold={view.alertThreshold}
        isLow={view.isLow}
        trend={view.trend}
      />

      <div className="acct-signin">
        <div style={{ minWidth: 0 }}>
          {error ? (
            <p className="acct-error">{error}</p>
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
          disabled={busy || state !== "claimable"}
        >
          {state === "claimable" ? "签到" : "已处理"}
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
          {switchable && (
            <button className="link" onClick={onSwitchClient} disabled={busy}>
              切换客户端账号
            </button>
          )}
          <button className="link" onClick={onLaunch} disabled={busy}>
            打开客户端
          </button>
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
    </article>
  );
}
