import { Fragment, useEffect, useRef, useState } from "react";
import { HOME_ID, type AgentGroup, type AgentVisibility, type ClaimMode, type ProviderView } from "../lib/types";
import { fmtPoints, fmtQuotaAmount, quotaUnit } from "../lib/api";
import { agentIcon, iconNeedsPad } from "../lib/agentIcons";
import VisibilityMenu from "./VisibilityMenu";

interface SidebarUpdateNotice {
  version: string;
  ready: boolean;
  onOpen: () => void;
}

interface Props {
  groups: AgentGroup[];
  selected: string;
  onSelect: (id: string) => void;
  onOpenHome: () => void;
  onOpenSettings: () => void;
  onOpenWebsite: (url: string) => void;
  /** 拖拽调序：把完整的新顺序交给上层落盘 */
  onReorder: (order: string[]) => void;
  /** 拖到另一区时手动调整该平台的分区 */
  onClaimModeChange: (providerId: string, mode: ClaimMode) => void;
  /** 全量平台（含被隐藏的），显示管理菜单用 */
  providers: ProviderView[];
  visibility: Record<string, AgentVisibility>;
  onVisibilityChange: (providerId: string, patch: Partial<AgentVisibility>) => void;
  /** 当前应用版本号 */
  version?: string;
  /** 可用更新提示（展示在侧栏底部、设置上方，避免挤占顶栏） */
  updateNotice?: SidebarUpdateNotice | null;
}

/** 设置页的虚拟 id（不是真实 provider）。 */
export const SETTINGS_ID = "__settings__";

const CHECKINBOX_WEBSITES = [
  { label: "官网", url: "https://skyblueeelabs.github.io/CheckinBox-Tools/" },
  { label: "镜像站", url: "https://checkinbox-tools-uwpjuqh846z.qoder.zone/" },
];

interface DragState {
  /** 被拖的 agent id */
  id: string;
  startY: number;
  grabOffsetY: number;
  /** 拖拽开始时各条目的中心 Y（视口坐标），用于计算落点 */
  centers: number[];
  listTop: number;
  /** 源条目的视口矩形（浮动卡的位置/宽度以它为基准） */
  itemRect: { left: number; top: number; width: number };
  /** 拖动偏移（px），超过阈值才算拖拽（与点击区分） */
  moved: boolean;
}

/** 侧栏按「每日福利获取方式」分成三区，区序固定，拖拽只在区内生效。 */
const SECTION_META: { mode: ClaimMode; label: string; hint: string }[] = [
  {
    mode: "auto",
    label: "自动签到",
    hint: "平台每天都发福利，本工具可以自动领取：WorkBuddy / MiniMax Code / Trae 免费用户可领；Qoder CN 限新注册或付费用户",
  },
  {
    mode: "manual",
    label: "需手动领取",
    hint: "有每日福利但只能在官方页面 / 客户端手动领：豆包受活动页风控限制（710010702）；ZCode 的 Tokens 活动暂时下线",
  },
  {
    mode: "none",
    label: "仅查看额度",
    hint: "没有每日签到，只展示额度：妙手是注册送 1,000 + 每月自动发 200",
  },
];
const MODE_RANK: Record<ClaimMode, number> = { auto: 0, manual: 1, none: 2 };

function claimModeOf(group: AgentGroup): ClaimMode {
  return group.provider.claimMode ?? (group.provider.supportsClaim ? "auto" : "manual");
}

/** 拖拽的视觉参数：移动超过该距离才算拖拽，低于它一律是点击 */
const DRAG_THRESHOLD = 10;

function AgentItemContent({ group }: { group: AgentGroup }) {
  const { provider, totalRemaining } = group;
  const icon = agentIcon(provider.id);
  const hasRemaining = totalRemaining > 0;
  const amount = hasRemaining ? fmtQuotaAmount(totalRemaining, provider.id) : "—";
  const exactAmount = quotaUnit(provider.id) === "Token" ? fmtPoints(totalRemaining) : amount;
  const mode = claimModeOf(group);
  const flagVerb = mode === "auto" ? "签" : "领";

  return (
    <>
      <span className="side-mark">
        {icon ? (
          <img
            src={icon}
            alt=""
            className={`mark-img ${iconNeedsPad(provider.id) ? "pad" : ""}`}
            draggable={false}
          />
        ) : (
          provider.name.slice(0, 1)
        )}
      </span>
      <span className="side-body">
        <span className="side-name">{provider.name}</span>
        <span className="side-sub">
          {group.accounts.length} 个账号
          {mode === "none" ? null : group.claimableCount > 0 ? (
            <em className="side-flag warn">{group.claimableCount} 待{flagVerb}</em>
          ) : group.claimedToday > 0 ? (
            <em className="side-flag ok">{group.claimedToday} 已{flagVerb}</em>
          ) : null}
        </span>
        <span
          className={`side-remaining ${hasRemaining ? "" : "zero"}`}
          title={hasRemaining ? `剩余 ${exactAmount}` : undefined}
        >
          {amount}
        </span>
      </span>
    </>
  );
}

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
 * 顺序由后端落盘，重启后保持；**跨区拖拽 = 手动调整该平台的分区**。
 */
export default function Sidebar({ groups, selected, onSelect, onOpenHome, onOpenSettings, onOpenWebsite, onReorder, onClaimModeChange, providers, visibility, onVisibilityChange, version, updateNotice }: Props) {
  const drag = useRef<DragState | null>(null);
  const lastDragEndAt = useRef(0);
  const [dragId, setDragId] = useState<string | null>(null);
  /** 平台显示管理菜单（眼睛按钮）开合 */
  const [visOpen, setVisOpen] = useState(false);
  /** 已按下但还没超过拖拽阈值的条目（不产生任何视觉）；null = 没有按下中 */
  const [pressedId, setPressedId] = useState<string | null>(null);
  const pressedRef = useRef<string | null>(null);
  const [dropIndex, setDropIndex] = useState<number | null>(null);
  /** 拖到别的区时预览的目标分区（松手才真正落盘） */
  const [pendingMode, setPendingMode] = useState<ClaimMode | null>(null);
  /** 浮动卡位置走 DOM 直改（ref）：setState 慢一帧会让快速拖动时
      浮动卡永远追不上鼠标（实测漂移逐帧累积 -10px/帧） */
  const ghostRef = useRef<HTMLDivElement | null>(null);
  const grabY = useRef(0);
  const listRef = useRef<HTMLElement | null>(null);

  // 侧栏显示顺序：先按「自动签到 → 需手动领取 → 仅查看额度」分三区，
  // 区内保持用户拖拽后的顺序（稳定排序）。
  const orderedGroups = [...groups].sort(
    (a, b) => MODE_RANK[claimModeOf(a)] - MODE_RANK[claimModeOf(b)],
  );

  // 全局监听在「按下」时就挂上（要在窗口任何位置跟踪 pointermove），
  // 但拖拽视觉等超过阈值才由 onMove 触发 —— 点击零视觉扰动
  useEffect(() => {
    if (!pressedId) return;

    /**
     * 拖拽落点：用按下时各条目的中心 Y 计算。
     * 落在别的区 = 手动调整分区（松手时落盘），索引再夹到目标区内。
     */
    const computeDrop = (clientY: number): { to: number; mode: ClaimMode } => {
      const state = drag.current;
      if (!state) return { to: 0, mode: "auto" };
      const from = orderedGroups.findIndex((g) => g.provider.id === state.id);
      if (from < 0) return { to: 0, mode: "auto" };

      // 各区的条目区间（centers 与 orderedGroups 一一对应）
      const bands = new Map<ClaimMode, { first: number; last: number }>();
      orderedGroups.forEach((g, i) => {
        const m = claimModeOf(g);
        const band = bands.get(m);
        if (band) band.last = i;
        else bands.set(m, { first: i, last: i });
      });
      const modes = SECTION_META.map((m) => m.mode).filter((m) => bands.has(m));
      let mode = modes[modes.length - 1];
      for (let k = 0; k < modes.length - 1; k += 1) {
        const cur = bands.get(modes[k])!;
        const next = bands.get(modes[k + 1])!;
        const boundary = (state.centers[cur.last] + state.centers[next.first]) / 2;
        if (clientY < boundary) {
          mode = modes[k];
          break;
        }
      }

      const raw = state.centers.filter((center) => clientY > center).length;
      // 原位移动补偿：拖过自己时索引偏移 1
      const moved = raw > from ? raw - 1 : raw;
      const target = bands.get(mode)!;
      return { to: Math.min(Math.max(moved, target.first), target.last), mode };
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
      // 位置每帧直改 DOM（零延迟）；落点变了才 setState
      const ghost = ghostRef.current;
      if (ghost) ghost.style.transform = `translateY(${e.clientY - state.startY}px)`;
      const { to, mode } = computeDrop(e.clientY);
      setDropIndex((prev) => (prev === to ? prev : to));
      setPendingMode((prev) => (prev === mode ? prev : mode));
    };

    const onUp = (e: PointerEvent) => {
      const state = drag.current;
      // 真·拖拽过（moved=true）才拦 click；未超过阈值的按下-松开是普通点击，
      // 不能记时间戳 —— 否则紧随其后的 click 会被 150ms 窗口误吞（点击失灵）
      if (state?.moved) {
        lastDragEndAt.current = Date.now();
        e.preventDefault();
        const { to, mode } = computeDrop(e.clientY);
        const ids = orderedGroups.map((g) => g.provider.id);
        const from = ids.indexOf(state.id);
        if (from >= 0) {
          if (to !== from) {
            ids.splice(from, 1);
            ids.splice(to, 0, state.id);
            onReorder(ids);
          }
          // 拖到了别的区：落盘新的分区（顺序与分区各存一份配置）
          if (mode !== claimModeOf(orderedGroups[from])) {
            onClaimModeChange(state.id, mode);
          }
        }
      }
      drag.current = null;
      pressedRef.current = null;
      setPressedId(null);
      setDragId(null);
      setDropIndex(null);
      setPendingMode(null);
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
    // 各条目中心点（含分组标题时高度不一致，不能再用固定 itemHeight 估算）
    const centers = Array.from(list.querySelectorAll<HTMLElement>(".side-agent")).map((el) => {
      const r = el.getBoundingClientRect();
      return r.top + r.height / 2;
    });
    drag.current = {
      id,
      startY: e.clientY,
      grabOffsetY: grabY.current,
      centers,
      listTop: list.getBoundingClientRect().top,
      itemRect: { left: rect.left, top: rect.top, width: rect.width },
      moved: false,
    };
    // 注意：这里**不** setDragId —— 骨架/浮动卡等拖拽视觉要等 pointermove
    // 超过阈值才出现（在 onMove 里 setDragId），点击全程零视觉扰动
    pressedRef.current = id;
    setPressedId(id);
  };

  /** 拖拽期间的显示顺序：被拖的条目从原位抽出，插到 dropIndex；跨区时按目标分区预览 */
  const displayGroups = (() => {
    const preview = (g: AgentGroup): AgentGroup =>
      dragId === g.provider.id && pendingMode && pendingMode !== claimModeOf(g)
        ? { ...g, provider: { ...g.provider, claimMode: pendingMode } }
        : g;
    if (!dragId || dropIndex === null) return orderedGroups;
    const from = orderedGroups.findIndex((g) => g.provider.id === dragId);
    if (from < 0) return orderedGroups;
    const rest = orderedGroups.filter((_, i) => i !== from);
    return [...rest.slice(0, dropIndex), orderedGroups[from], ...rest.slice(dropIndex)].map(preview);
  })();

  const draggedGroup = dragId ? orderedGroups.find((g) => g.provider.id === dragId) : null;

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
        <span className="sidebar-count">
          {groups.length}
          {providers.length > groups.length ? `/${providers.length}` : ""}
        </span>
        <button
          type="button"
          className="sidebar-vis-btn"
          data-vis-toggle
          aria-label="管理平台显示"
          aria-haspopup="dialog"
          aria-expanded={visOpen}
          title="管理平台显示（侧栏 / 首页）"
          onClick={() => setVisOpen((v) => !v)}
        >
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7-10-7-10-7z" />
            <circle cx="12" cy="12" r="3" />
          </svg>
        </button>
        {visOpen && (
          <VisibilityMenu
            providers={providers}
            visibility={visibility}
            onChange={onVisibilityChange}
            onClose={() => setVisOpen(false)}
          />
        )}
      </div>

      <nav className={`sidebar-list ${dragId ? "is-dragging" : ""}`} ref={listRef}>
        {groups.length === 0 && (
          <p className="empty-state">
            {providers.length === 0 ? "还没有账号" : "所有平台已隐藏，点上方眼睛图标恢复"}
          </p>
        )}
        {displayGroups.map((group, i) => {
          const isActive = group.provider.id === selected;
          const isDragging = dragId === group.provider.id;
          const mode = claimModeOf(group);
          const sectionStart = i === 0 || claimModeOf(displayGroups[i - 1]) !== mode;
          return (
            <Fragment key={group.provider.id}>
              {sectionStart && (
                <div
                  className="side-section"
                  role="presentation"
                  title={SECTION_META.find((m) => m.mode === mode)?.hint}
                >
                  {SECTION_META.find((m) => m.mode === mode)?.label}
                </div>
              )}
              <button
                className={`side-item side-agent ${isActive ? "active" : ""} ${isDragging ? "dragging" : ""} ${dragId && !isDragging ? "shifting" : ""}`}
                style={{ ["--agent-accent" as string]: group.provider.accent }}
                onClick={() => {
                  // 刚完成拖拽的松手会带一个 click，用「距 pointerup 的时间」滤掉
                  if (Date.now() - lastDragEndAt.current < 150) return;
                  onSelect(group.provider.id);
                }}
                onPointerDown={startDrag(group.provider.id)}
                title="点击选择 · 拖拽调序（同区内）"
              >
                <AgentItemContent group={group} />
              </button>
            </Fragment>
          );
        })}
      </nav>

      {/* 跟手的浮动卡：被拖条目的实时预览（键盘可达性：仅指针拖拽时渲染） */}
      {draggedGroup && drag.current && (
        <div
          ref={ghostRef}
          className="drag-ghost side-agent"
          style={{
            ["--ghost-left" as string]: `${drag.current.itemRect.left}px`,
            ["--ghost-top" as string]: `${drag.current.itemRect.top}px`,
            ["--ghost-width" as string]: `${drag.current.itemRect.width}px`,
          }}
          aria-hidden
        >
          <AgentItemContent group={draggedGroup} />
        </div>
      )}

      {updateNotice && (
        <div className="sidebar-update-slot">
          <button
            type="button"
            className={`sidebar-update-card ${updateNotice.ready ? "ready" : ""}`}
            onClick={updateNotice.onOpen}
          >
            <span className="sidebar-update-body">
              <span className="sidebar-update-title">
                <span className="sidebar-update-dot" aria-hidden="true" />
                <span>{updateNotice.ready ? `v${updateNotice.version} 已就绪` : `发现新版本 v${updateNotice.version}`}</span>
              </span>
              <span className="sidebar-update-sub">
                {updateNotice.ready ? "点击立即重启安装" : "点击查看更新内容"}
              </span>
            </span>
            <span className="sidebar-update-action" aria-hidden="true">
              {updateNotice.ready ? "重启 ›" : "更新 ›"}
            </span>
          </button>
        </div>
      )}

      <div className="sidebar-foot">
        <button
          className={`side-item side-settings ${selected === SETTINGS_ID ? "active" : ""}`}
          onClick={onOpenSettings}
          title="设置"
        >
          <span className="settings-mark">
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
              <circle cx="12" cy="12" r="3" />
              <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
            </svg>
          </span>
          <span className="side-body">
            <span className="side-name">设置</span>
            <span className="side-sub">同步 / 调度 / 数据</span>
          </span>
        </button>
        <div className="sidebar-meta">
          <span className="sidebar-version num" title={version ? `当前版本 v${version}` : undefined}>
            {version ? `v${version}` : ""}
          </span>
          <div className="sidebar-websites" role="group" aria-label="CheckinBox 网站">
            {CHECKINBOX_WEBSITES.map(({ label, url }) => (
              <a
                key={url}
                className="link"
                href={url}
                title={url}
                onClick={(event) => {
                  event.preventDefault();
                  onOpenWebsite(url);
                }}
              >
                {label} <span aria-hidden="true">↗</span>
              </a>
            ))}
          </div>
        </div>
      </div>
    </aside>
  );
}
