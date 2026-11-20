import type { AgentGroup, LiveFreeModels } from "../lib/types";
import { api, fmtQuotaAmount, quotaLabels } from "../lib/api";
import { WELFARE_KIND_TEXT, WELFARE_META, FREE_MODELS, FREE_STATUS_TEXT, welfareOpenState, welfareOf } from "../lib/welfare";
import TimeInput from "./TimeInput";
import { agentIcon, iconNeedsPad } from "../lib/agentIcons";
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
  /** 发起设备授权 */
  onAddAccount: (providerId: string) => void;
  /** 把客户端切换成这个账号（改写客户端凭据文件） */
  onSwitchClient: (providerId: string, accountId: string) => void;
  onAlertChange: (providerId: string, value: number) => void;
  /** 改「换号同步聊天记录」开关（仅 WorkBuddy 显示） */
  onChatSyncChange: (providerId: string, enabled: boolean) => void;
  /** WorkBuddy 实时免费模型核对状态 */
  liveFree?: LiveFreeModels | null;
  onRefreshFree?: () => void;
}

/**
 * 各平台「活动刷新时刻」的提示。
 * 时间窗如果配在刷新之前，到点只会拿到「未开始」，然后被标记成今天已处理。
 */
const RESET_HINT: Record<string, string> = {
  qoder: "Qoder 的每日活动 10:00（UTC+8）才刷新，时间窗要放在 10:00 之后",
};

/**
 * 「换号同步聊天记录」的说明（借鉴 Cockpit 的 share_sessions_on_switch）。
 * 两个客户端的存储形态不同，措辞分开写：
 *   * WorkBuddy 会话按账号 uid 分目录 —— 开关做的是「合并到目标账号名下」；
 *   * Qoder 的聊天库在本机共享 —— 切号时默认保留，不需要账号迁移开关。
 */
const CHAT_SYNC_HINT: Record<string, string> = {
  workbuddy:
    "开启后，切换客户端账号时会把当前账号的本地会话合并进目标账号；关闭则不碰聊天记录，各账号只看到自己名下的会话",
};

/** 与 MiniMax 自动同步的关系说明（开关只在 WorkBuddy 上渲染）。 */
const CHAT_SYNC_MINIMAX_NOTE =
  "MiniMax 不受此开关影响：它的会话存在服务端，客户端登录即自动同步，优先于本地记录";

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
  onSwitchClient,
  onAlertChange,
  onScheduleChange,
  onChatSyncChange,
  liveFree,
  onRefreshFree,
}: Props) {
  const { provider, accounts, schedule } = group;
  const accent = provider.accent;
  const panelIcon = agentIcon(provider.id);
  // Zcode 的额度是 Token（3 亿级），其它平台是积分 —— 标签与格式跟着单位走
  const labels = quotaLabels(provider.id);

  // 客户端实际登录的账号，和工具里选中的当前账号对不对得上
  const clientAccount = schedule.clientAccount;
  const activeId = accounts.find((a) => a.isActive)?.account.id;
  const mismatch =
    !!clientAccount && !!activeId && clientAccount.accountId !== activeId;
  const formatMinute = (minute: number) => `${String(Math.floor(minute / 60)).padStart(2, "0")}:${String(minute % 60).padStart(2, "0")}`;
  const free = FREE_MODELS[provider.id];
  const openState = welfareOpenState(provider.id);
  const welfare = welfareOf(group);
  const meta = WELFARE_META[provider.id];
  // 缺省视为已就绪；Trae 首版先展示「即将开放」。
  const oauthReady = provider.oauthReady !== false;

  return (
    <section className="panel agent-panel" style={{ ["--agent-accent" as string]: accent }}>
      <header className="panel-head">
        <div className="panel-title">
          <span className="panel-mark">
            <img
              src={panelIcon}
              alt=""
              className={`mark-img ${iconNeedsPad(provider.id) ? "pad" : ""}`}
              draggable={false}
            />
          </span>
          <div>
            <h2>{provider.name}</h2>
            <p className="panel-sub">
              {accounts.length} 个账号 · 今日
              {provider.supportsClaim ? "已签" : "已领取"} {schedule.todayClaimed} 个
              {schedule.todayClaimable > 0
                ? ` · ${schedule.todayClaimable} 个${provider.supportsClaim ? "可签" : "待领取"}`
                : ""}
            </p>
            {clientAccount && (
              <p className={`client-line ${mismatch ? "warn" : ""}`}>
                客户端当前登录 <b>{clientAccount.label}</b>
                {!clientAccount.inPool && <span className="client-note">（不在账号池里）</span>}
                {mismatch && <span className="client-note"> · 与当前账号不一致</span>}
              </p>
            )}
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
          {provider.supportsOAuth && (
            <button
              className="btn small"
              onClick={() => onAddAccount(provider.id)}
              disabled={busy || authPending || !oauthReady}
              title={
                !oauthReady
                  ? "Trae CN 浏览器授权即将开放，先使用「从本机导入」"
                  : undefined
              }
            >
              {!oauthReady
                ? "添加账号（即将开放）"
                : authPending
                  ? "等待授权…"
                  : "添加账号"}
            </button>
          )}
        </div>
      </header>

      {/* 福利统一口径：第一行积分/额度，第二行免费模型；时效敏感标注核对月份 */}
      {meta && (
        <div className="welfare-brief" role="note" aria-label={`${provider.name} 福利`}>
          <div className="wb-row">
            <span className="wb-label">积分/额度</span>
            {welfare.benefits.map((b) => (
              <span className="wb-benefit" key={b.kind}>
                <span className="wb-cadence">{WELFARE_KIND_TEXT[b.kind]}</span>
                <span className="wb-credits">{b.label}</span>
                {b.modeText && <span className={`wb-mode ${b.mode}`}>{b.modeText}</span>}
              </span>
            ))}
            {openState && (
              <span className={`wb-open ${openState.open ? "on" : ""}`}>{openState.label}</span>
            )}
            {welfare.note && <span className="wb-note">{welfare.note}</span>}
            {welfare.extraNote && (
              <span className="wb-note">{welfare.extraNote}</span>
            )}
            {welfare.docs && (
              <button
                type="button"
                className="wb-docs"
                onClick={() => void api.openExternal(welfare.docs!.url)}
              >
                {welfare.docs.label} ↗
              </button>
            )}
          </div>
          {free && (
          <div className="wb-row">
            <span className="wb-label">免费模型</span>
            {provider.id === "workbuddy" ? (
              <>
                {liveFree?.models
                  .filter((m) => m.free)
                  .map((m) => (
                    <span key={m.id} className="wb-chip" title={`计费倍率 ${m.credits || "未知"}`}>
                      {m.name}
                    </span>
                  ))}
                {liveFree &&
                  !liveFree.loading &&
                  !liveFree.error &&
                  liveFree.models.length > 0 &&
                  !liveFree.models.some((m) => m.free) && (
                    <span className="wb-none">本期目录暂无免费模型</span>
                  )}
                {!liveFree && <span className="wb-none">有免费模型 · 待实时核对</span>}
                {liveFree?.loading && <span className="wb-none">核对中…</span>}
                {liveFree?.error && (
                  <span className="wb-none">有免费模型 · 实时核对不可用</span>
                )}
                {liveFree && !liveFree.loading && !liveFree.error && (
                  <span className="wb-live">
                    实时{" "}
                    {new Date(liveFree.fetchedAt).toLocaleTimeString("zh-CN", {
                      hour: "2-digit",
                      minute: "2-digit",
                    })}
                  </span>
                )}
                {onRefreshFree && (
                  <button
                    className={`wb-refresh ${liveFree?.loading ? "is-loading" : ""}`}
                    onClick={onRefreshFree}
                    disabled={liveFree?.loading}
                    aria-label="核对最新免费模型"
                    title="从 WorkBuddy 模型目录实时核对当前免费模型"
                  >
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                      <path d="M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8" />
                      <path d="M21 3v5h-5" />
                    </svg>
                  </button>
                )}
              </>
            ) : free.models?.length ? (
              free.models.map((m) => (
                <span key={m} className="wb-chip">{m}</span>
              ))
            ) : (
              <span className="wb-none">{free.note ?? FREE_STATUS_TEXT[free.status]}</span>
            )}
            {free.asOf && <span className="wb-asof">核对 {free.asOf}</span>}
          </div>
          )}
        </div>
      )}

      <details className="agent-options">
        <summary>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" aria-hidden="true"><path d="M4 7h7m4 0h5M4 17h3m4 0h9" /><circle cx="13" cy="7" r="2" /><circle cx="9" cy="17" r="2" /></svg>
          <span>自动化设置</span>
          <span className="agent-options-summary">
            {provider.supportsClaim ? (schedule.enabled ? `${formatMinute(schedule.startMinute)}–${formatMinute(schedule.endMinute)} 自动签到` : "自动签到已关闭") : "官方客户端领取"}
            {schedule.chatSyncSupported && ` · 聊天同步${schedule.chatSyncEnabled ? "已开启" : "已关闭"}`}
          </span>
          <span className="agent-options-caret" aria-hidden="true">⌄</span>
        </summary>
        <div className="agent-options-body">
      {/* 换号同步聊天记录（WorkBuddy 按账号分片；Qoder 本地历史默认共享） */}
      {schedule.chatSyncSupported && (
        <div className="chat-sync">
          <label className="switch">
            <input
              type="checkbox"
              checked={schedule.chatSyncEnabled}
              onChange={(e) => onChatSyncChange(provider.id, e.target.checked)}
            />
            <span>换号同步聊天记录</span>
          </label>
          <span className="chat-sync-hint" title={CHAT_SYNC_MINIMAX_NOTE}>
            {CHAT_SYNC_HINT[provider.id] ?? "切换客户端账号时保留各账号的本地会话"}
            <em className="chat-sync-note"> · {CHAT_SYNC_MINIMAX_NOTE}</em>
          </span>
        </div>
      )}
      {/* 这个 agent 自己的自动签到。
          不代领的平台（Zcode：领取要过官方客户端的阿里云验证码）没有可自动
          执行的东西，换成一句说明，避免用户以为开了就会自动到账。 */}
      {provider.supportsClaim ? (
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
            <TimeInput
              value={schedule.startMinute}
              onCommit={(m) => onScheduleChange(provider.id, { startMinute: m })}
              ariaLabel={`${provider.name} 时间窗起`}
            />
            <span className="sched-dash">→</span>
            <TimeInput
              value={schedule.endMinute}
              onCommit={(m) => onScheduleChange(provider.id, { endMinute: m })}
              ariaLabel={`${provider.name} 时间窗止`}
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
      ) : (
        <div className="sched-hint">
          {provider.name} 每日额度需在官方客户端领取
          {WELFARE_META[provider.id]?.timing ? `（${WELFARE_META[provider.id].timing}）` : ""}
          ，本工具只展示余额并提醒
        </div>
      )}
        </div>
      </details>

      {accounts.length === 0 ? (
        <div className="empty-state">
          <p>
            还没有账号。
            {provider.supportsDiscovery
              ? "点「从本机导入」读取本机客户端登录态。"
              : "点「添加账号」走一次浏览器授权。"}
          </p>
        </div>
      ) : (
        <>
          <div className="quota-stats summary">
            <div className="qstat">
              <span className="q-label">{labels.granted}</span>
              <span className="q-value lg">{fmtQuotaAmount(group.totalGranted, provider.id)}</span>
            </div>
            <div className="qstat">
              <span className="q-label">{labels.remaining}</span>
              <span className="q-value lg">{fmtQuotaAmount(group.totalRemaining, provider.id)}</span>
            </div>
            <div className="qstat">
              <span className="q-label">{provider.supportsClaim ? "今日已签" : "今日已领取"}</span>
              <span className="q-value lg">
                {schedule.todayClaimed} / {accounts.length}
              </span>
            </div>
          </div>

          <div className="acct-grid">
            {accounts.map((view) => (
              <AccountCard
                key={`${view.account.provider}:${view.account.id}`}
                view={view}
                busy={busy}
                quotaLoading={quotaLoadingId === `${view.account.provider}:${view.account.id}`}
                switchable={schedule.switchable}
                onSwitchClient={() =>
                  onSwitchClient(view.account.provider, view.account.id)
                }
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
