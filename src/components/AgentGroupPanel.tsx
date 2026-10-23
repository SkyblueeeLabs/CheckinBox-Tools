import type { AgentGroup } from "../lib/types";
import { fmtPoints, formatMinute, parseMinute } from "../lib/api";
import AccountCard from "./AccountCard";

interface Props {
  group: AgentGroup;
  busy: boolean;
  quotaLoadingId: string | null;
  authPending: boolean;
  onCheckin: (providerId: string, accountId: string, label: string) => void;
  onRefreshQuota: (providerId: string, accountId: string) => void;
  onSetActive: (providerId: string, accountId: string) => void;
  onLaunch: (providerId: string, accountId: string) => void;
  onRemove: (providerId: string, accountId: string, label: string) => void;
  onDiscover: (providerId: string) => void;
  onScheduleChange: (
    providerId: string,
    patch: { enabled?: boolean; startMinute?: number; endMinute?: number }
  ) => void;
  /** 发起设备授权（qoder / qwenwork 共用同一套 URL，只是账号不同） */
  onAddAccount: (providerId: string) => void;
  onAlertChange: (providerId: string, value: number) => void;
}

/**
 * 各平台「活动刷新时刻」的提示。
 * 时间窗如果配在刷新之前，到点只会拿到「未开始」，然后被标记成今天已处理。
 */
const RESET_HINT: Record<string, string> = {
  qoder: "Qoder 的每日活动 10:00（UTC+8）才刷新，时间窗要放在 10:00 之后",
  qwenwork: "千问办公的每日签到活动当前是停用状态，暂不会自动领取",
};

/**
 * 单个 agent 的面板。
 *
 * 自动签到配置**属于这个 agent 自己**（开关 + 时间窗），不再有全局时间窗 ——
 * 这样 Qoder 这种 10:00 才刷新活动的平台可以单独配窗口。
 */
export default function AgentGroupPanel({
  group,
  busy,
  quotaLoadingId,
  authPending,
  onCheckin,
  onRefreshQuota,
  onSetActive,
  onLaunch,
  onRemove,
  onDiscover,
  onAddAccount,
  onAlertChange,
  onScheduleChange,
}: Props) {
  const { provider, accounts, schedule } = group;
  const accent = provider.accent;

  return (
    <section className="panel" style={{ ["--agent-accent" as string]: accent }}>
      <header className="panel-head">
        <div className="panel-title">
          <span className="panel-mark" />
          <div>
            <h2>{provider.name}</h2>
            <p className="panel-sub">
              {accounts.length} 个账号 · 今日已签 {schedule.todayClaimed} 个
              {schedule.todayClaimable > 0 ? ` · ${schedule.todayClaimable} 个可签` : ""}
            </p>
          </div>
        </div>

        <div className="panel-actions">
          {provider.hasClient && (
            <button className="btn small" onClick={() => onLaunch(provider.id, "")} disabled={busy}>
              打开 {provider.name}
            </button>
          )}
          {provider.supportsDiscovery && (
            <button
              className="btn small"
              onClick={() => onDiscover(provider.id)}
              disabled={busy}
            >
              从本机导入
            </button>
          )}
          {provider.id === "qoder" || provider.id === "qwenwork" ? (
            <button
              className="btn small"
              onClick={() => onAddAccount(provider.id)}
              disabled={busy || authPending}
            >
              {authPending ? "等待授权…" : "添加账号"}
            </button>
          ) : null}
        </div>
      </header>

      {/* 这个 agent 自己的自动签到 */}
      <div className="sched">
        <label className="switch">
          <input
            type="checkbox"
            checked={schedule.enabled}
            onChange={(e) => onScheduleChange(provider.id, { enabled: e.target.checked })}
          />
          <span>自动签到</span>
        </label>

        <span className="sched-label">时间窗</span>
        <span className="sched-win">
          <input
            value={formatMinute(schedule.startMinute)}
            onChange={(e) => {
              const m = parseMinute(e.target.value);
              if (m !== null) onScheduleChange(provider.id, { startMinute: m });
            }}
          />
          <span className="sched-dash">→</span>
          <input
            value={formatMinute(schedule.endMinute)}
            onChange={(e) => {
              const m = parseMinute(e.target.value);
              if (m !== null) onScheduleChange(provider.id, { endMinute: m });
            }}
          />
        </span>
        <span className="sched-hint">窗内每账号随机错峰执行</span>
        {RESET_HINT[provider.id] ? (
          <span className="sched-hint warn">{RESET_HINT[provider.id]}</span>
        ) : null}

        <span className="sched-label">预警</span>
        <span className="sched-win">
          <input
            className="sched-threshold"
            type="number"
            min={0}
            value={schedule.alertThreshold ?? 200}
            onChange={(e) => {
              const v = Number(e.target.value);
              if (Number.isFinite(v) && v >= 0) onAlertChange(provider.id, Math.round(v));
            }}
          />
          <span className="sched-hint">剩余低于此值标红</span>
        </span>
      </div>

      {accounts.length === 0 ? (
        <div className="empty">
          <p>
            还没有账号。
            {provider.supportsDiscovery
              ? "点「从本机导入」读取本机客户端登录态。"
              : "点「添加账号」走一次浏览器授权。"}
          </p>
        </div>
      ) : (
        <>
          <div className="quota-stats" style={{ marginTop: 14, gap: 24 }}>
            <div className="qstat">
              <span className="q-label">剩余积分</span>
              <span className="q-value" style={{ fontSize: 18, color: "var(--ok)" }}>
                {fmtPoints(group.totalRemaining)}
              </span>
            </div>
            <div className="qstat">
              <span className="q-label">累计发放</span>
              <span className="q-value" style={{ fontSize: 18 }}>
                {fmtPoints(group.totalGranted)}
              </span>
            </div>
          </div>

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
                onSetActive={() =>
                  onSetActive(view.account.provider, view.account.id)
                }
                onLaunch={() => onLaunch(view.account.provider, view.account.id)}
                onRemove={() =>
                  onRemove(view.account.provider, view.account.id, view.account.label)
                }
              />
            ))}
          </div>
        </>
      )}
    </section>
  );
}
