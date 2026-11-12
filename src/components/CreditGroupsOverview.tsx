import type { CreditGroup, CreditItem } from "../lib/types";
import { fmtCredits, fmtDateTime } from "../lib/api";
import { groupExpiring } from "../lib/credits";

interface Props {
  groups: CreditGroup[];
  /** 有效批次（算「包含即将到期的积分」标记用） */
  items: CreditItem[];
  /** 打开指定分组的资源包 */
  onGroup?: (kind: string) => void;
}

/**
 * WorkBuddy 四分组积分概览（套餐基础 / 套餐赠送 / 平台奖励 / 购买）。
 * 数字来自服务端 summary，口径与小卡片一致。
 */
export default function CreditGroupsOverview({ groups, items, onGroup }: Props) {
  const now = Date.now();
  return (
    <div className="credit-groups">
      {groups.map((g) => {
        const remainPct = g.total > 0 ? Math.min(100, Math.floor((g.remaining / g.total) * 100)) : 0;
        const usedPct = g.total > 0 ? Math.min(100, (g.used / g.total) * 100) : 0;
        const expiring = groupExpiring(items, g.kind, now);
        return (
          <div className="credit-group" data-kind={g.kind} key={g.kind}>
            <div className="credit-group-line">
              <strong>{g.name}</strong>
              <span className="credit-group-remaining" title={`剩余 ${fmtCredits(g.remaining)} 积分`}>
                {fmtCredits(g.remaining)}
              </span>
            </div>
            <div className="credit-group-line muted">
              <span>
                已使用 {fmtCredits(g.used)}/{fmtCredits(g.total)}
              </span>
              <span>剩余{remainPct}%</span>
            </div>
            <div className="credit-group-track" aria-hidden="true">
              <span style={{ width: `${usedPct}%` }} />
            </div>
            {g.timeMs ? (
              <div className="credit-group-time muted">
                {g.usesExpiry ? "最近到期" : "下次刷新"} {fmtDateTime(g.timeMs)}
              </div>
            ) : null}
            {expiring && <span className="credit-group-expiry">包含即将到期的积分</span>}
            {onGroup && (
              <button
                className="link credit-group-action"
                aria-label={`查看详情 · ${g.name}`}
                onClick={() => onGroup(g.kind)}
              >
                查看详情 ›
              </button>
            )}
          </div>
        );
      })}
    </div>
  );
}
