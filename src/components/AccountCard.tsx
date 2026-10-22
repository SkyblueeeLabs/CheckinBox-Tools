import type { AccountView } from "../lib/types";

interface Props {
  view: AccountView;
  busy: boolean;
  onCheckin: () => void;
  onRemove: () => void;
}

const STATE_TEXT: Record<string, string> = {
  claimable: "可签到",
  claimed: "今日已签",
  notAvailable: "暂无可签",
  unknown: "状态未知",
};

export default function AccountCard({ view, busy, onCheckin, onRemove }: Props) {
  const { status, error, account } = view;
  const state = status?.state ?? "unknown";

  return (
    <article className={`card ${state}`}>
      <div className="card-top">
        <div className="who">
          <span className="avatar">{(account.label || "?").slice(0, 1)}</span>
          <div>
            <strong>{account.label}</strong>
            <span className="origin">{account.origin}</span>
          </div>
        </div>
        <span className={`badge ${state}`}>{STATE_TEXT[state] ?? state}</span>
      </div>

      <div className="card-body">
        {error ? (
          <p className="error">{error}</p>
        ) : status ? (
          <p className="message">{status.message}</p>
        ) : (
          <p className="message dim">正在读取状态…</p>
        )}

        <div className="meta">
          {status && status.dayIndex ? <span>第 {status.dayIndex}/7 天</span> : null}
          {status && status.streakDays > 0 ? <span>连签 {status.streakDays} 天</span> : null}
          {status && status.bonusPoints > 0 ? (
            <span className="bonus">额外 +{status.bonusPoints}</span>
          ) : null}
        </div>
      </div>

      <footer className="card-foot">
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
