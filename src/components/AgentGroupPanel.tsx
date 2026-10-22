import type { AgentGroup } from "../lib/types";
import { fmtPoints } from "../lib/api";
import AccountCard from "./AccountCard";

interface Props {
  group: AgentGroup;
  busy: boolean;
  quotaLoadingId: string | null;
  authPending: boolean;
  onCheckin: (providerId: string, accountId: string, label: string) => void;
  onRefreshQuota: (providerId: string, accountId: string) => void;
  onDiscover: () => void;
  onAddAccount: () => void;
  onRemove: (providerId: string, accountId: string, label: string) => void;
}

/**
 * 单个 agent 分组的主面板。
 * 顶部是该 agent 的积分汇总条，下面是账号列表。
 */
export default function AgentGroupPanel({
  group,
  busy,
  quotaLoadingId,
  authPending,
  onCheckin,
  onRefreshQuota,
  onDiscover,
  onAddAccount,
  onRemove,
}: Props) {
  const { provider, accounts, totalRemaining, totalGranted } = group;

  return (
    <section className="group" style={{ ["--accent" as string]: provider.accent }}>
      <header className="group-head">
        <div className="group-title">
          <span className="group-icon">{provider.name.slice(0, 1)}</span>
          <div>
            <h2>{provider.name}</h2>
            <p className="group-sub">
              {accounts.length} 个账号 · 今日已签 {group.claimedToday} 个
            </p>
          </div>
        </div>

        <div className="group-actions">
          <button className="ghost" onClick={onDiscover} disabled={busy}>
            从本机客户端导入
          </button>
          {provider.id === "qoder" && (
            <button className="ghost" onClick={onAddAccount} disabled={busy || authPending}>
              {authPending ? "等待授权…" : "添加账号"}
            </button>
          )}
        </div>
      </header>

      {accounts.length > 0 && (
        <div className="group-totals">
          <div className="total-cell">
            <span className="total-num accent">{fmtPoints(totalRemaining)}</span>
            <span className="total-key">剩余积分</span>
          </div>
          <div className="total-cell">
            <span className="total-num">{fmtPoints(totalGranted)}</span>
            <span className="total-key">累计发放</span>
          </div>
          <div className="total-cell">
            <span className="total-num">
              {accounts.filter((a) => a.status?.state === "claimable").length}
            </span>
            <span className="total-key">待签到</span>
          </div>
        </div>
      )}

      {accounts.length === 0 ? (
        <div className="group-empty">
          <p>
            还没有账号。
            {provider.supportsDiscovery
              ? "点「从本机客户端导入」读取本机登录态。"
              : "点「添加账号」走一次浏览器授权。"}
          </p>
        </div>
      ) : (
        <div className="acct-list">
          {accounts.map((view) => (
            <AccountCard
              key={`${view.account.provider}:${view.account.id}`}
              view={view}
              busy={busy}
              quotaLoading={quotaLoadingId === `${view.account.provider}:${view.account.id}`}
              onCheckin={() =>
                onCheckin(view.account.provider, view.account.id, view.account.label)
              }
              onRefreshQuota={() =>
                onRefreshQuota(view.account.provider, view.account.id)
              }
              onRemove={() =>
                onRemove(view.account.provider, view.account.id, view.account.label)
              }
            />
          ))}
        </div>
      )}
    </section>
  );
}
