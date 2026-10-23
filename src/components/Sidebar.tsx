import type { AgentGroup } from "../lib/types";
import { fmtPoints } from "../lib/api";
import { agentIcon } from "../lib/agentIcons";

interface Props {
  groups: AgentGroup[];
  selected: string;
  onSelect: (id: string) => void;
  onOpenSettings: () => void;
}

/** 设置页的虚拟 id（不是真实 provider）。 */
export const SETTINGS_ID = "__settings__";

/**
 * 左侧导航。
 *
 * 每个平台一个圆角方块显示**官方图标**，右侧跟剩余积分（等宽数字）。
 */
export default function Sidebar({ groups, selected, onSelect, onOpenSettings }: Props) {
  return (
    <aside className="sidebar">
      <div className="sidebar-head">
        <span className="sidebar-title">Agents</span>
        <span className="sidebar-count">{groups.length}</span>
      </div>

      <nav className="sidebar-list">
        {groups.length === 0 && <p className="empty">还没有账号</p>}
        {groups.map((group) => {
          const isActive = group.provider.id === selected;
          const remaining = group.totalRemaining;
          const icon = agentIcon(group.provider.id);
          const fallback = group.provider.name.slice(0, 1);
          return (
            <button
              key={group.provider.id}
              className={`side-item ${isActive ? "active" : ""}`}
              style={{ ["--agent-accent" as string]: group.provider.accent }}
              onClick={() => onSelect(group.provider.id)}
            >
              <span className="side-mark">
                {icon ? (
                  <img src={icon} alt="" className="mark-img" draggable={false} />
                ) : (
                  fallback
                )}
              </span>
              <span className="side-body">
                <span className="side-name">{group.provider.name}</span>
                <span className="side-sub">
                  {group.accounts.length} 个账号
                  {group.claimableCount > 0 ? (
                    <em className="side-flag warn">{group.claimableCount} 待签</em>
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
        <button
          className={`side-item side-settings ${selected === SETTINGS_ID ? "active" : ""}`}
          onClick={onOpenSettings}
        >
          <span className="settings-mark">设</span>
          <span className="side-body">
            <span className="side-name">设置</span>
            <span className="side-sub">同步 / 调度 / 数据</span>
          </span>
        </button>
        <p>积分仅用于平台内 AI 任务消耗</p>
      </div>
    </aside>
  );
}
