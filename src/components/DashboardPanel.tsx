import type { AgentGroup, Overview } from "../lib/types";
import { fmtPoints } from "../lib/api";
import { agentIcon, iconNeedsPad } from "../lib/agentIcons";

interface Props {
  overview: Overview;
  groups: AgentGroup[];
  /** 点击某个 agent 卡 → 跳到该平台的面板 */
  onOpenAgent: (id: string) => void;
}

/**
 * 首页总览。
 *
 * 上面是全局关键指标（剩余/发放总积分、账号总数、今日已签），
 * 下面是每个 agent 一张卡：官方图标、积分、账号数、今日签到状态，
 * 点击卡片跳到该平台的详情面板。
 */
export default function DashboardPanel({ overview, groups, onOpenAgent }: Props) {
  const totalRemaining = groups.reduce((s, g) => s + g.totalRemaining, 0);
  const totalGranted = groups.reduce((s, g) => s + g.totalGranted, 0);
  const accountCount = overview.accounts.length;
  const claimedCount = groups.reduce((s, g) => s + g.claimedToday, 0);
  const claimableCount = groups.reduce((s, g) => s + g.claimableCount, 0);

  const stats = [
    { label: "剩余总积分", value: fmtPoints(totalRemaining), accent: true },
    { label: "累计发放总积分", value: fmtPoints(totalGranted) },
    { label: "账号总数", value: String(accountCount), sub: `${groups.length} 个平台` },
    {
      label: "今日已签",
      value: `${claimedCount} / ${accountCount}`,
      sub: claimableCount > 0 ? `${claimableCount} 个待签` : "全部处理完毕",
    },
  ];

  return (
    <section className="panel dash">
      <header className="panel-head">
        <div className="panel-title">
          <div>
            <h2>总览</h2>
            <p className="panel-sub">
              {groups.length} 个平台 · {accountCount} 个账号
            </p>
          </div>
        </div>
      </header>

      {/* ---------- 全局关键指标 ---------- */}
      <div className="dash-stats">
        {stats.map((s) => (
          <div key={s.label} className={`dash-stat ${s.accent ? "accent" : ""}`}>
            <span className="dash-stat-label">{s.label}</span>
            <span className="dash-stat-value">{s.value}</span>
            {s.sub && <span className="dash-stat-sub">{s.sub}</span>}
          </div>
        ))}
      </div>

      {/* ---------- 各 agent 卡 ---------- */}
      <div className="dash-agents">
        {groups.map((g) => {
          const icon = agentIcon(g.provider.id);
          const allClaimed = g.accounts.length > 0 && g.claimedToday === g.accounts.length;
          const stateText = (() => {
            if (g.accounts.length === 0) return "暂无账号";
            if (g.claimableCount > 0) return `${g.claimableCount} 个待签`;
            if (allClaimed) return "今日全部已签";
            if (g.claimedToday > 0) return `已签 ${g.claimedToday}/${g.accounts.length}`;
            return "今日未处理";
          })();
          const stateTone = (() => {
            if (g.accounts.length === 0) return "dim";
            if (g.claimableCount > 0) return "warn";
            if (allClaimed) return "ok";
            return "dim";
          })();
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
                  <b className="num">{fmtPoints(g.totalRemaining)}</b>
                </span>
                <span className="dash-agent-stat">
                  <i>发放</i>
                  <b className="num">{fmtPoints(g.totalGranted)}</b>
                </span>
              </div>
            </button>
          );
        })}
      </div>
    </section>
  );
}
