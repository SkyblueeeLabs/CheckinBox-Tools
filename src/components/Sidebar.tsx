import { useEffect, useRef, useState } from "react";
import { HOME_ID, type AgentGroup } from "../lib/types";
import { fmtPoints } from "../lib/api";
import { agentIcon, iconNeedsPad } from "../lib/agentIcons";

interface Props {
  groups: AgentGroup[];
  selected: string;
  onSelect: (id: string) => void;
  onOpenHome: () => void;
  onOpenSettings: () => void;
  /** 拖拽调序：把完整的新顺序交给上层落盘 */
  onReorder: (order: string[]) => void;
}

/** 设置页的虚拟 id（不是真实 provider）。 */
export const SETTINGS_ID = "__settings__";

interface DragState {
  /** 被拖的 agent id */
  id: string;
  startY: number;
  grabOffsetY: number;
  /** 每个条目的高度（拖拽期间固定） */
  itemHeight: number;
  listTop: number;
  /** 源条目的视口矩形（浮动卡的位置/宽度以它为基准） */
  itemRect: { left: number; top: number; width: number };
  /** 拖动偏移（px），超过阈值才算拖拽（与点击区分） */
  moved: boolean;
}

/** 拖拽的视觉参数：移动超过该距离才算拖拽，低于它一律是点击 */
const DRAG_THRESHOLD = 10;

/**
 * 左侧导航。
 *
 * 每个平台一个圆角方块显示**官方图标**，右侧跟剩余积分（等宽数字）。
 * 列表支持拖拽调序：pointer 事件自实现（WebView2 里原生 HTML5 DnD 不可靠）。
 * 视觉参考 dnd-kit 的 sortable 最佳实践：
 *   - 被拖条目原位变成**骨架插槽**（虚线框 + 降透明度）；
 *   - 跟手的**浮动卡**（放大 1.03 + 倾斜 2.5° + 大阴影）；
 *   - 其余条目用 transform 过渡**平滑让位**；
 *   - 松手浮动卡消失，条目在落位处淡入（scale 1.06 → 1）。
 * 顺序由后端落盘，重启后保持。
 */
export default function Sidebar({ groups, selected, onSelect, onOpenHome, onOpenSettings, onReorder }: Props) {
  const drag = useRef<DragState | null>(null);
  const lastDragEndAt = useRef(0);
  const [dragId, setDragId] = useState<string | null>(null);
  /** 已按下但还没超过拖拽阈值的条目（不产生任何视觉）；null = 没有按下中 */
  const [pressedId, setPressedId] = useState<string | null>(null);
  const pressedRef = useRef<string | null>(null);
  const [dropIndex, setDropIndex] = useState<number | null>(null);
  /** 浮动卡位置走 DOM 直改（ref）：setState 慢一帧会让快速拖动时
      浮动卡永远追不上鼠标（实测漂移逐帧累积 -10px/帧） */
  const ghostRef = useRef<HTMLDivElement | null>(null);
  const grabY = useRef(0);
  const listRef = useRef<HTMLElement | null>(null);

  // 全局监听在「按下」时就挂上（要在窗口任何位置跟踪 pointermove），
  // 但拖拽视觉等超过阈值才由 onMove 触发 —— 点击零视觉扰动
  useEffect(() => {
    if (!pressedId) return;

    const computeDropIndex = (clientY: number): number => {
      const state = drag.current;
      const list = listRef.current;
      if (!state || !list) return 0;
      // 相对列表顶部的偏移决定插入位置（半高以上算上方）
      const rel = clientY - state.listTop + state.itemHeight / 2;
      const idx = Math.max(0, Math.min(groups.length, Math.floor(rel / state.itemHeight)));
      const from = groups.findIndex((g) => g.provider.id === state.id);
      // 原位移动补偿：拖过自己时索引偏移 1
      return idx > from ? idx - 1 : idx;
    };

    const onMove = (e: PointerEvent) => {
      const state = drag.current;
      if (!state) return;
      if (!state.moved) {
        if (Math.abs(e.clientY - state.startY) < DRAG_THRESHOLD) return;
        state.moved = true;
        // 超过阈值：此刻才进入拖拽视觉（骨架 + 浮动卡 + 让位过渡）
        setDragId(state.id);
      }
      // 位置每帧直改 DOM（零延迟）；dropIndex 变了才 setState
      const ghost = ghostRef.current;
      if (ghost) ghost.style.transform = `translateY(${e.clientY - state.startY}px)`;
      const idx = computeDropIndex(e.clientY);
      setDropIndex((prev) => (prev === idx ? prev : idx));
    };

    const onUp = (e: PointerEvent) => {
      const state = drag.current;
      // 真·拖拽过（moved=true）才拦 click；未超过阈值的按下-松开是普通点击，
      // 不能记时间戳 —— 否则紧随其后的 click 会被 150ms 窗口误吞（点击失灵）
      if (state?.moved) {
        lastDragEndAt.current = Date.now();
        e.preventDefault();
        const to = computeDropIndex(e.clientY);
        const ids = groups.map((g) => g.provider.id);
        const from = ids.indexOf(state.id);
        if (from >= 0 && to !== from) {
          ids.splice(from, 1);
          ids.splice(to, 0, state.id);
          onReorder(ids);
        }
      }
      drag.current = null;
      pressedRef.current = null;
      setPressedId(null);
      setDragId(null);
      setDropIndex(null);
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("pointercancel", onUp);
    };

    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onUp);
    return () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("pointercancel", onUp);
    };
    // groups 在拖拽期间不会变（轮询期间 groups 引用可能变，但顺序不变）
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pressedId]);

  const startDrag = (id: string) => (e: React.PointerEvent) => {
    // 只响应主键；右键/中键不拖
    if (e.button !== 0) return;
    const list = listRef.current;
    const item = (e.currentTarget as HTMLElement).closest(".side-item");
    if (!list || !item) return;
    const rect = item.getBoundingClientRect();
    grabY.current = e.clientY - rect.top;
    drag.current = {
      id,
      startY: e.clientY,
      grabOffsetY: grabY.current,
      itemHeight: rect.height + 2, // 含 gap
      listTop: list.getBoundingClientRect().top,
      itemRect: { left: rect.left, top: rect.top, width: rect.width },
      moved: false,
    };
    // 注意：这里**不** setDragId —— 骨架/浮动卡等拖拽视觉要等 pointermove
    // 超过阈值才出现（在 onMove 里 setDragId），点击全程零视觉扰动
    pressedRef.current = id;
    setPressedId(id);
  };

  /** 拖拽期间的显示顺序：被拖的条目从原位抽出，插到 dropIndex */
  const displayGroups = (() => {
    if (!dragId || dropIndex === null) return groups;
    const from = groups.findIndex((g) => g.provider.id === dragId);
    if (from < 0) return groups;
    const rest = groups.filter((_, i) => i !== from);
    return [...rest.slice(0, dropIndex), groups[from], ...rest.slice(dropIndex)];
  })();

  const draggedGroup = dragId ? groups.find((g) => g.provider.id === dragId) : null;

  return (
    <aside className="sidebar">
      <button
        className={`side-item side-home ${selected === HOME_ID ? "active" : ""}`}
        onClick={onOpenHome}
        title="账号与积分总览"
      >
        <span className="side-mark home-mark">
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
            <path d="M3 10.5 12 3l9 7.5" />
            <path d="M5 9.5V21h14V9.5" />
            <path d="M9.5 21v-6h5v6" />
          </svg>
        </span>
        <span className="side-body">
          <span className="side-name">首页</span>
          <span className="side-sub">总览 · 关键指标</span>
        </span>
        <span className="side-remaining num">{groups.length}</span>
      </button>

      <div className="sidebar-head">
        <span className="sidebar-title">Agents</span>
        <span className="sidebar-count">{groups.length}</span>
      </div>

      <nav className={`sidebar-list ${dragId ? "is-dragging" : ""}`} ref={listRef}>
        {groups.length === 0 && <p className="empty-state">还没有账号</p>}
        {displayGroups.map((group) => {
          const isActive = group.provider.id === selected;
          const remaining = group.totalRemaining;
          const icon = agentIcon(group.provider.id);
          const fallback = group.provider.name.slice(0, 1);
          const isDragging = dragId === group.provider.id;
          return (
            <button
              key={group.provider.id}
              className={`side-item ${isActive ? "active" : ""} ${isDragging ? "dragging" : ""} ${dragId && !isDragging ? "shifting" : ""}`}
              style={{ ["--agent-accent" as string]: group.provider.accent }}
              onClick={() => {
                // 刚完成拖拽的松手会带一个 click，用「距 pointerup 的时间」滤掉
                if (Date.now() - lastDragEndAt.current < 150) return;
                onSelect(group.provider.id);
              }}
              onPointerDown={startDrag(group.provider.id)}
              title="点击选择 · 拖拽调序"
            >
              <span className="side-mark">
                {icon ? (
                  <img
                    src={icon}
                    alt=""
                    className={`mark-img ${iconNeedsPad(group.provider.id) ? "pad" : ""}`}
                    draggable={false}
                  />
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
              <span className={`side-remaining ${remaining > 0 ? "" : "zero"}`}>
                {remaining > 0 ? fmtPoints(remaining) : "—"}
              </span>
            </button>
          );
        })}
      </nav>

      {/* 跟手的浮动卡：被拖条目的实时预览（键盘可达性：仅指针拖拽时渲染） */}
      {draggedGroup && drag.current && (
        <div
          ref={ghostRef}
          className="drag-ghost"
          style={{
            ["--ghost-left" as string]: `${drag.current.itemRect.left}px`,
            ["--ghost-top" as string]: `${drag.current.itemRect.top}px`,
            ["--ghost-width" as string]: `${drag.current.itemRect.width}px`,
          }}
          aria-hidden
        >
          <span className="side-mark">
            {agentIcon(draggedGroup.provider.id) ? (
              <img
                src={agentIcon(draggedGroup.provider.id)}
                alt=""
                className={`mark-img ${iconNeedsPad(draggedGroup.provider.id) ? "pad" : ""}`}
                draggable={false}
              />
            ) : (
              draggedGroup.provider.name.slice(0, 1)
            )}
          </span>
          <span className="side-body">
            <span className="side-name">{draggedGroup.provider.name}</span>
            <span className="side-sub">
              {draggedGroup.accounts.length} 个账号
              {draggedGroup.claimableCount > 0 ? (
                <em className="side-flag warn">{draggedGroup.claimableCount} 待签</em>
              ) : draggedGroup.claimedToday > 0 ? (
                <em className="side-flag ok">{draggedGroup.claimedToday} 已签</em>
              ) : null}
            </span>
          </span>
          <span className={`side-remaining ${draggedGroup.totalRemaining > 0 ? "" : "zero"}`}>
            {draggedGroup.totalRemaining > 0 ? fmtPoints(draggedGroup.totalRemaining) : "—"}
          </span>
        </div>
      )}

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
