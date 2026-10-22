import type { AgentGroup } from "../lib/types";
import { fmtPoints } from "../lib/api";

interface Props {
  groups: AgentGroup[];
  selected: string;
  activeCount: number;
  onSelect: (id: string) => void;
}

/**
 * 左侧 agent 分组栏 —— 布局参考 Cockpit Tools：
 * 每个 agent 一行，显示名称、账号数、剩余积分汇总。
 */
export default function Sidebar({ groups, selected, activeCount, onSelect }: Props) {
  return (
    <aside className="sidebar">
      <div className="sidebar-head">
        <span className="sidebar-title">Agent 分组</span>
        <span className="sidebar-count">{activeCount} 个在线</span>
      </div>

      <nav className="sidebar-list">
        {groups.length === 0 && <p className="sidebar-empty">还没有账号</p>}
        {groups.map((group) => {
          const isActive = group.provider.id === selected;
          return (
            <button
              key={group.provider.id}
              className={`side-item ${isActive ? "active" : ""}`}
              onClick={() => onSelect(group.provider.id)}
            >
              <span
                className="side-icon"
                style={{ background: group.provider.accent }}
              >
                {group.provider.name.slice(0, 1)}
              </span>
              <span className="side-body">
                <span className="side-name">{group.provider.name}</span>
                <span className="side-sub">
                  {group.accounts.length} 个账号
                  {group.claimableCount > 0 ? (
                    <em className="side-badge warn">{group.claimableCount} 可签</em>
                  ) : group.claimedToday > 0 ? (
                    <em className="side-badge ok">{group.claimedToday} 已签</em>
                  ) : null}
                </span>
              </span>
              <span className="side-quota">
                <span className="side-quota-num">
                  {group.totalRemaining > 0 ? fmtPoints(group.totalRemaining) : "—"}
                </span>
                <span className="side-quota-label">剩余</span>
              </span>
            </button>
          );
        })}
      </nav>

      <div className="sidebar-foot">
        <p>积分仅用于平台内 AI 任务消耗</p>
      </div>
    </aside>
  );
}
