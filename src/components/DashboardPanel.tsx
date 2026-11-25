import type { ReactNode } from "react";
import { agentVisibilityOf, type AgentGroup, type AgentVisibility, type Overview } from "../lib/types";
import { fmtCompact, fmtPoints, fmtQuotaAmount, quotaUnit } from "../lib/api";
import { EXPIRY_WINDOW_DAYS, expiringAccounts } from "../lib/expiry";
import { agentIcon, iconNeedsPad } from "../lib/agentIcons";
import { welfareOf } from "../lib/welfare";

interface Props {
  overview: Overview;
  groups: AgentGroup[];
  /** 点击某个 agent 卡 → 跳到该平台的面板 */
  onOpenAgent: (id: string) => void;
  announcement?: ReactNode;
  /** 每个平台的显示开关；没列出的平台全部显示 */
  visibility: Record<string, AgentVisibility>;
}

/**
 * 首页总览。
 *
 * 上面是全局关键指标（剩余/发放总积分、账号总数、今日已签），
 * 下面是每个 agent 一张卡：官方图标、积分、账号数、今日签到状态，
 * 点击卡片跳到该平台的详情面板。
 */
export default function DashboardPanel({ overview, groups, onOpenAgent, announcement, visibility }: Props) {
  // 全局指标保持全量口径（与顶栏「今日已签 N/M」一致）；
  // 首页隐藏只影响下面的到期列表和 agent 卡片，不改汇总数字。
  const homeVisible = (id: string) => agentVisibilityOf(visibility, id).home;
  const visibleGroups = groups.filter((g) => homeVisible(g.provider.id));
  const pointsGroups = groups.filter((g) => quotaUnit(g.provider.id) !== "Token");
  const tokenGroups = groups.filter((g) => quotaUnit(g.provider.id) === "Token");
  const pointsRemaining = pointsGroups.reduce((s, g) => s + g.totalRemaining, 0);
  const pointsGranted = pointsGroups.reduce((s, g) => s + g.totalGranted, 0);
  const tokenRemaining = tokenGroups.reduce((s, g) => s + g.totalRemaining, 0);
  const tokenGranted = tokenGroups.reduce((s, g) => s + g.totalGranted, 0);
  const hasTokenQuota = tokenRemaining > 0 || tokenGranted > 0;
  const tokenProviderLabel = tokenGroups
    .filter((g) => g.totalRemaining > 0 || g.totalGranted > 0)
    .map((g) => g.provider.name)
    .join("/") || "Token 平台";
  const accountCount = overview.accounts.length;
  const claimedCount = groups.reduce((s, g) => s + g.claimedToday, 0);
  const autoClaimableCount = groups
    .filter((g) => g.provider.supportsClaim)
    .reduce((s, g) => s + g.claimableCount, 0);
  const manualClaimableCount = groups
    .filter((g) => !g.provider.supportsClaim)
    .reduce((s, g) => s + g.claimableCount, 0);
  const expiring = expiringAccounts(overview.accounts).filter((x) =>
    homeVisible(x.view.account.provider)
  );
  const totalExpiring = expiring.reduce((s, x) => s + x.credits.remaining, 0);
  // 到期汇总的计量单位：全部同一单位就说它，混合（ZCode Token + 其它积分）就标出来
  const expiringUnits = [...new Set(expiring.map((x) => quotaUnit(x.view.account.provider)))];
  const expiringUnitText = expiringUnits.length === 1 ? expiringUnits[0] : expiringUnits.join("+");

  const claimSub = (() => {
    if (autoClaimableCount > 0 && manualClaimableCount > 0) {
      return `${autoClaimableCount} 个可签 · ${manualClaimableCount} 个待手领`;
    }
    if (autoClaimableCount > 0) return `${autoClaimableCount} 个待签`;
    if (manualClaimableCount > 0) return `${manualClaimableCount} 个待手领`;
    return "全部处理完毕";
  })();

  // 通用积分与 Token 额度拆开展示，避免 3 亿 Token 把几千积分淹没成 300.0M
  const stats = [
    {
      label: hasTokenQuota ? "剩余通用积分" : "剩余总积分",
      value: fmtCompact(pointsRemaining),
      full: fmtPoints(pointsRemaining),
      sub: hasTokenQuota
        ? tokenRemaining > 0
          ? `另 ${tokenProviderLabel} ${fmtQuotaAmount(tokenRemaining, tokenGroups[0].provider.id)} Token`
          : "不含 Token 额度"
        : undefined,
      tone: undefined as "warn" | undefined,
    },
    {
      label: hasTokenQuota ? "累计发放积分" : "累计发放总积分",
      value: fmtCompact(pointsGranted),
      full: fmtPoints(pointsGranted),
      sub: hasTokenQuota ? "不含 Token 额度" : undefined,
      tone: undefined as "warn" | undefined,
    },
    {
      label: "账号总数",
      value: String(accountCount),
      full: undefined,
      sub: `${groups.length} 个平台`,
      tone: undefined as "warn" | undefined,
    },
    {
      label: "今日已签",
      value: `${claimedCount} / ${accountCount}`,
      full: undefined,
      sub: claimSub,
      tone: (autoClaimableCount > 0 || manualClaimableCount > 0 ? "warn" : undefined) as "warn" | undefined,
    },
  ];

  return (
    <section className="panel dash">
      {/* 顶部不再重复「总览」标题 —— 顶栏已经标了当前页面，面板头纯属多余
          （平台/账号数在侧栏与顶栏「今日已签 N/M」里都能看到） */}

      {announcement}

      {/* ---------- 全局关键指标 ---------- */}
      <div className="dash-stats">
        {stats.map((s) => (
          <div key={s.label} className="dash-stat">
            <span className="dash-stat-label">{s.label}</span>
            <span className="dash-stat-value" title={s.full}>{s.value}</span>
            {s.sub && <span className={`dash-stat-sub ${s.tone ?? ""}`}>{s.sub}</span>}
          </div>
        ))}
      </div>

      {/* ---------- 即将到期（按紧迫度排序） ---------- */}
      {expiring.length > 0 && (
        <div className="dash-expiring">
          <div className="dash-expiring-head">
            <h3>{EXPIRY_WINDOW_DAYS} 天内到期</h3>
            <span>
              {expiring.length} 个账号 · {fmtCompact(totalExpiring)} {expiringUnitText}
            </span>
          </div>
          <div className="dash-expiring-list">
            {expiring.map(({ view, credits }) => {
              const icon = agentIcon(view.account.provider);
              return (
                <button
                  key={`${view.account.provider}:${view.account.id}`}
                  className="dash-expiring-row"
                  style={{ ["--agent-accent" as string]: view.accent }}
                  onClick={() => onOpenAgent(view.account.provider)}
                  title={`${fmtQuotaAmount(credits.remaining, view.account.provider)} ${quotaUnit(view.account.provider)}将在 ${credits.daysLeft} 天内到期，点击去处理`}
                >
                  <span className="side-mark">
                    {icon ? (
                      <img
                        src={icon}
                        alt=""
                        className={`mark-img ${iconNeedsPad(view.account.provider) ? "pad" : ""}`}
                        draggable={false}
                      />
                    ) : (
                      view.account.label.slice(0, 1)
                    )}
                  </span>
                  <span className="dash-expiring-name">
                    <strong>{view.account.label}</strong>
                    <span>{view.providerName}</span>
                  </span>
                  <span className="dash-expiring-pts">
                    <b className="num">{fmtQuotaAmount(credits.remaining, view.account.provider)}</b>
                    <span className={credits.daysLeft <= 3 ? "urgent" : undefined}>
                      {credits.daysLeft} 天
                    </span>
                  </span>
                </button>
              );
            })}
          </div>
        </div>
      )}

      {/* ---------- 各 agent 卡 ---------- */}
      <div className="dash-agents">
        {visibleGroups.length === 0 && groups.length > 0 && (
          <p className="empty-state">
            所有平台已在首页隐藏。点侧栏 Agents 旁的眼睛图标可恢复显示。
          </p>
        )}
        {visibleGroups.map((g) => {
          const icon = agentIcon(g.provider.id);
          const allClaimed = g.accounts.length > 0 && g.claimedToday === g.accounts.length;
          // 不代领的平台（ZCode）说「领取」，其它说「签」
          const verb = g.provider.supportsClaim ? "签" : "领取";
          const stateText = (() => {
            if (g.accounts.length === 0) return "暂无账号";
            if (g.claimableCount > 0) return `${g.claimableCount} 个待${verb}`;
            if (allClaimed) return `今日全部已${verb}`;
            if (g.claimedToday > 0) return `已${verb} ${g.claimedToday}/${g.accounts.length}`;
            return "今日未处理";
          })();
          const stateTone = (() => {
            if (g.accounts.length === 0) return "dim";
            if (g.claimableCount > 0) return "warn";
            if (allClaimed) return "ok";
            return "dim";
          })();
          // 福利画像：每天大概多少、能不能自动领、不能领的节奏（一次性/时间窗）
          const welfare = welfareOf(g);
          return (
            <button
              key={g.provider.id}
              className="dash-agent"
              style={{ ["--agent-accent" as string]: g.provider.accent }}
              onClick={() => onOpenAgent(g.provider.id)}
              title={`打开 ${g.provider.name} 面板`}
            >
              <header className="dash-agent-head">
                <span className="side-mark">
                  {icon ? (
                    <img
                      src={icon}
                      alt=""
                      className={`mark-img ${iconNeedsPad(g.provider.id) ? "pad" : ""}`}
                      draggable={false}
                    />
                  ) : (
                    g.provider.name.slice(0, 1)
                  )}
                </span>
                <span className="dash-agent-name">
                  <strong>{g.provider.name}</strong>
                  <span className={`dash-agent-state ${stateTone}`}>{stateText}</span>
                </span>
                <span className="dash-agent-count">{g.accounts.length} 个账号</span>
              </header>
              <div className="dash-agent-stats">
                <span className="dash-agent-stat">
                  <i>剩余</i>
                  <b className="num" title={fmtPoints(g.totalRemaining)}>
                    {fmtQuotaAmount(g.totalRemaining, g.provider.id)}
                  </b>
                </span>
                <span className="dash-agent-stat">
                  <i>发放</i>
                  <b className="num" title={fmtPoints(g.totalGranted)}>
                    {fmtQuotaAmount(g.totalGranted, g.provider.id)}
                  </b>
                </span>
              </div>

              <div className="dash-agent-welfare" title={welfare.title}>
                {welfare.benefits.map((b) => (
                  <span className="welfare-benefit" key={b.kind}>
                    <span className="welfare-amount">{b.label}</span>
                    {b.modeText && (
                      <span className={`welfare-mode ${b.mode}`}>{b.modeText}</span>
                    )}
                  </span>
                ))}
                {welfare.note && <span className="welfare-note">{welfare.note}</span>}
                {welfare.extraNote && (
                  <span className="welfare-note">{welfare.extraNote}</span>
                )}
                {welfare.freeText && (
                  <span className="welfare-note">{welfare.freeText}</span>
                )}
              </div>
            </button>
          );
        })}
      </div>
    </section>
  );
}
