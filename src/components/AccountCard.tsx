import type { AccountView } from "../lib/types";
import QuotaCard from "./QuotaCard";

interface Props {
  view: AccountView;
  busy: boolean;
  quotaLoading: boolean;
  onCheckin: () => void;
  onRefreshQuota: () => void;
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
  onCheckin,
  onRefreshQuota,
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
        <span className={`acct-badge ${state}`}>{STATE_TEXT[state] ?? state}</span>
      </header>

      <QuotaCard
        quota={view.quota}
        error={view.quotaError}
        loading={quotaLoading}
        onRefresh={onRefreshQuota}
      />

      <div className="acct-signin">
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

      <footer className="acct-foot">
        <button
          className="primary small"
          onClick={onCheckin}
          disabled={busy || state !== "claimable"}
        >
          {state === "claimable" ? "立即签到" : "已处理"}
        </button>
        <button className="link" onClick={onRemove} disabled={busy}>
          移除
        </button>
      </footer>
    </article>
  );
}
