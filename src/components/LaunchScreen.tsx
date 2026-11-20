import { useEffect } from "react";
import { agentVisibilityOf, type AgentGroup, type AgentVisibility, type Overview } from "../lib/types";
import { buildGroups } from "../lib/groups";
import { fmtPoints } from "../lib/api";
import { agentIcon } from "../lib/agentIcons";

interface Props {
  overview: Overview | null;
  loadError: string | null;
  busy: boolean;
  /** 每个平台的显示开关；launch=false 的平台蛋不上封面 */
  visibility: Record<string, AgentVisibility>;
  onClaimAll: () => void;
  onClaimProvider: (providerId: string) => void;
  onOpenProvider: (providerId: string) => void;
  onEnter: () => void;
  onRetry: () => void;
  /** 提供时为「随时看一眼」浮层模式：Esc / 返回 回到原页面 */
  onBack?: () => void;
}

type EggState = "loading" | "done" | "ready" | "manual" | "wait" | "empty" | "error";

function eggStateOf(group: AgentGroup, loading: boolean): EggState {
  if (loading) return "loading";
  if (group.accounts.length === 0) return "empty";
  if (group.accounts.every((a) => !!a.error)) return "error";
  if (group.claimableCount > 0) return group.provider.supportsClaim ? "ready" : "manual";
  if (group.claimedToday > 0) return "done";
  return "wait";
}

function eggStatusText(group: AgentGroup, state: EggState): string {
  switch (state) {
    case "loading":
      return "待孵化";
    case "done":
      return `已领 ${group.claimedToday}/${group.accounts.length}`;
    case "ready":
      return `${group.claimableCount} 枚待领`;
    case "manual":
      return `${group.claimableCount} 枚待领取`;
    case "empty":
      return "空巢";
    case "error":
      return "读取失败";
    default:
      return "今日无可领";
  }
}

/** 漂浮数据尘埃：确定性布点，避免每次渲染随机跳 */
const MOTES = Array.from({ length: 18 }, (_, i) => ({
  left: `${(i * 53 + 11) % 97}%`,
  top: `${(i * 37 + 7) % 89}%`,
  size: 2 + (i % 3),
  duration: 7 + (i % 5) * 2.5,
  delay: -(i * 0.9),
}));

/**
 * 启动页「今日的赛博鸡蛋，领了嘛？」。
 *
 * 每个平台一枚赛博蛋：已领 = 裂壳金蛋，可领 = 青光脉动，需手动 = 青圈待领取，
 * 空巢 / 无可领 = 暗蛋。领蛋动作复用主界面的签到管线，状态随 overview 实时翻转。
 * 每次进程启动展示一次，进入主界面后不再出现（托盘唤醒不重弹）；
 * 顶栏蛋图标可随时以浮层模式再看一眼（onBack），Esc / 返回 回到原页面。
 * 蛋行按 visibility.launch 过滤；已领 / 待领 / 积分统计保持全量口径。
 */
export default function LaunchScreen({
  overview,
  loadError,
  busy,
  visibility,
  onClaimAll,
  onClaimProvider,
  onOpenProvider,
  onEnter,
  onRetry,
  onBack,
}: Props) {
  const loading = !overview;
  const groups = overview ? buildGroups(overview) : [];
  const visibleGroups = groups.filter(
    (g) => agentVisibilityOf(visibility, g.provider.id).launch
  );
  const autoReady = groups
    .filter((g) => g.provider.supportsClaim)
    .reduce((s, g) => s + g.claimableCount, 0);
  const claimableTotal = groups.reduce((s, g) => s + g.claimableCount, 0);
  const claimedCount = overview?.claimedCount ?? 0;
  const totalCount = overview?.totalCount ?? 0;

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") (onBack ?? onEnter)();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onEnter, onBack]);

  const answer = loading
    ? "正在数蛋…"
    : loadError
      ? "孵化舱信号不好，先重试或直接进入。"
      : totalCount === 0
        ? "巢里还没蛋，先去各平台导入账号。"
        : claimableTotal === 0 && claimedCount > 0
          ? "领完了！今日孵化舱满员运转。"
          : claimedCount === 0
            ? `一枚都没领，${claimableTotal} 枚蛋在等你下手。`
            : `还有 ${claimableTotal} 枚蛋没领，已入袋 ${claimedCount} 枚。`;

  return (
    <div className="launch">
      <div className="launch-bg" aria-hidden="true">
        <svg className="launch-egg-art e1" viewBox="0 0 120 150">
          <path d="M60 8 C88 8 108 44 108 82 C108 116 86 142 60 142 C34 142 12 116 12 82 C12 44 32 8 60 8 Z" />
        </svg>
        <svg className="launch-egg-art e2" viewBox="0 0 120 150">
          <path d="M60 8 C88 8 108 44 108 82 C108 116 86 142 60 142 C34 142 12 116 12 82 C12 44 32 8 60 8 Z" />
        </svg>
        <svg className="launch-egg-art e3" viewBox="0 0 120 150">
          <path d="M60 8 C88 8 108 44 108 82 C108 116 86 142 60 142 C34 142 12 116 12 82 C12 44 32 8 60 8 Z" />
        </svg>
        <div className="launch-motes">
          {MOTES.map((m, i) => (
            <span
              key={i}
              className={`mote-c${i % 4}`}
              style={{
                left: m.left,
                top: m.top,
                width: m.size,
                height: m.size,
                animationDuration: `${m.duration}s`,
                animationDelay: `${m.delay}s`,
              }}
            />
          ))}
        </div>
      </div>

      <div className="launch-inner">
      <header className="launch-head">
        <p className="launch-eyebrow">CHECKINBOX · 赛博孵化舱</p>
        <h1 className="launch-title">
          今日的<span className="launch-hl">赛博鸡蛋</span>，领了嘛？
        </h1>
        <p className="launch-answer" role="status">{answer}</p>
      </header>

      {loadError && (
        <div className="launch-error" role="alert">
          <span>账号与积分加载失败：{loadError}</span>
          <button className="launch-retry" onClick={onRetry}>重试</button>
        </div>
      )}

      <div className="egg-row">
        {groups.length === 0 && !loading && (
          <p className="launch-empty">巢里还没蛋 —— 进入应用后从本机客户端导入账号。</p>
        )}
        {visibleGroups.length === 0 && groups.length > 0 && !loading && (
          <p className="launch-empty">
            所有平台的蛋已在封面隐藏。点侧栏 Agents 旁的眼睛图标可恢复显示。
          </p>
        )}
        {visibleGroups.map((group, i) => {
          const state = eggStateOf(group, loading);
          const icon = agentIcon(group.provider.id);
          const pid = group.provider.id;
          return (
            <div
              key={pid}
              className={`egg-pod is-${state} ${busy && state === "ready" ? "is-hatching" : ""}`}
              style={{ ["--egg-accent" as string]: group.provider.accent, ["--i" as string]: i }}
            >
              <svg className="egg-svg" viewBox="0 0 120 150" aria-hidden="true">
                <ellipse className="egg-base" cx="60" cy="141" rx="33" ry="6" />
                <path
                  className="egg-shell"
                  d="M60 8 C88 8 108 44 108 82 C108 116 86 142 60 142 C34 142 12 116 12 82 C12 44 32 8 60 8 Z"
                />
                <path
                  className="egg-shade"
                  d="M14 92 C20 122 38 142 60 142 C82 142 100 122 106 92 C98 116 82 130 60 130 C38 130 22 116 14 92 Z"
                />
                <path className="egg-shine" d="M40 28 C30 42 26 58 27 72" />
                {state === "done" && (
                  <>
                    <ellipse className="egg-yolk" cx="60" cy="88" rx="27" ry="21" />
                    <ellipse className="egg-yolk-hl" cx="52" cy="82" rx="9" ry="6" />
                    <path className="egg-crack" d="M28 62 L44 54 L54 66 L68 54 L80 66 L94 58" />
                  </>
                )}
                {icon && state !== "done" && (
                  <image href={icon} x="38" y="56" width="44" height="44" className="egg-icon" />
                )}
                {state === "done" && (
                  <g className="egg-badge">
                    <circle cx="94" cy="32" r="14" />
                    <path d="M88 32 l4.5 4.5 L101 27" />
                  </g>
                )}
                {state === "ready" && (
                  <path className="egg-spark" d="M98 52 l3 7 7 3 -7 3 -3 7 -3 -7 -7 -3 7 -3 z" />
                )}
              </svg>
              <strong className="egg-name">{group.provider.name}</strong>
              <span className="egg-status">{eggStatusText(group, state)}</span>
              {state === "ready" && (
                <button className="egg-claim" onClick={() => onClaimProvider(pid)} disabled={busy}>
                  领
                </button>
              )}
              {state === "manual" && (
                <button className="egg-claim manual" onClick={() => onOpenProvider(pid)}>
                  去领取
                </button>
              )}
              {state === "empty" && (
                <button className="egg-claim manual" onClick={() => onOpenProvider(pid)}>
                  去导入
                </button>
              )}
              {state === "done" && <span className="egg-chip egg-chip-ok">已入袋</span>}
              {state === "wait" && <span className="egg-chip egg-chip-dim">明日再来</span>}
              {state === "error" && (
                <button className="egg-claim manual" onClick={onRetry}>重试</button>
              )}
            </div>
          );
        })}
        {loading &&
          [0, 1, 2, 3, 4].map((i) => (
            <div key={`sk-${i}`} className="egg-pod is-loading" style={{ ["--i" as string]: i }}>
              <svg className="egg-svg" viewBox="0 0 120 150" aria-hidden="true">
                <path
                  className="egg-shell skeleton-egg"
                  d="M60 8 C88 8 108 44 108 82 C108 116 86 142 60 142 C34 142 12 116 12 82 C12 44 32 8 60 8 Z"
                />
              </svg>
              <span className="egg-status">待孵化</span>
            </div>
          ))}
      </div>

      <div className="launch-stats">
        <div className="launch-stat">
          <b className="num">{claimedCount}</b>
          <span>/ {totalCount} 枚已领</span>
        </div>
        <div className="launch-stat points">
          <b className="num">+{fmtPoints(overview?.todayPoints ?? 0)}</b>
          <span>今日积分</span>
        </div>
        <div className="launch-stat ready">
          <b className="num">{claimableTotal}</b>
          <span>枚待领</span>
        </div>
      </div>

      <div className="launch-actions">
        <button
          className="launch-cta"
          onClick={onClaimAll}
          disabled={busy || loading || autoReady === 0}
        >
          {busy ? "孵化中…" : autoReady === 0 && !loading ? "今日蛋已领完" : "一键领蛋"}
        </button>
        {onBack ? (
          <button className="launch-enter" onClick={onBack}>
            <span aria-hidden="true">←</span> 返回原页面
          </button>
        ) : (
          <button className="launch-enter" onClick={onEnter}>
            进入 CheckinBox <span aria-hidden="true">→</span>
          </button>
        )}
      </div>

      <p className="launch-foot">
        {onBack
          ? "浮层盖在当前页之上 · Esc 或「返回」回到原页面，导航状态不丢"
          : "隐藏与进入都不影响后台：自动签到与同步照常运行 · Esc 直接进入"}
      </p>
      </div>
    </div>
  );
}