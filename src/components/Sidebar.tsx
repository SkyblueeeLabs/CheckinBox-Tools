import type { AgentGroup } from "../lib/types";
import { fmtPoints } from "../lib/api";

interface Props {
  groups: AgentGroup[];
  selected: string;
  onSelect: (id: string) => void;
}

/**
 * 左侧 agent 列表。
 * 每行：强调色小方块 + 名称 + 账号数/状态角标 + 右侧剩余积分（等宽数字）。
 */
export default function Sidebar({ groups, selected, onSelect }: Props) {
  return (
    <aside className="sidebar">
      <div className="sidebar-head">
        <span className="micro">Agents</span>
        <span className="micro">{groups.length}</span>
      </div>

      <nav className="sidebar-list">
        {groups.length === 0 && <p className="empty">还没有账号</p>}
        {groups.map((group) => {
          const isActive = group.provider.id === selected;
          const remaining = group.totalRemaining;
          return (
            <button
              key={group.provider.id}
              className={`side-item ${isActive ? "active" : ""}`}
              onClick={() => onSelect(group.provider.id)}
            >
              <span
                className="side-dot"
                style={{ background: group.provider.accent }}
              />
              <span className="side-body">
                <span className="side-name">{group.provider.name}</span>
                <span className="side-sub">
                  {group.accounts.length} 个账号
                  {group.claimableCount > 0 ? (
                    <em className="side-flag warn">
                      {group.claimableCount} 待签
                    </em>
                  ) : group.claimedToday > 0 ? (
                    <em className="side-flag ok">{group.claimedToday} 已签</em>
                  ) : null}
                </span>
              </span>
              <span className={`side-remaining ${remaining > 0 ? "" : "empty"}`}>
                {remaining > 0 ? fmtPoints(remaining) : "—"}
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
