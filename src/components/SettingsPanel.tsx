import { useMemo, useState, type ReactNode } from "react";
import type { AppConfig, CloseAction, Overview } from "../lib/types";
import TimeInput from "./TimeInput";
import BackupPanel from "./BackupPanel";

const CLOSE_OPTIONS: Array<{ value: CloseAction; label: string; desc: string }> = [
  { value: "ask", label: "每次询问", desc: "弹确认框，自己选最小化还是退出" },
  { value: "tray", label: "最小化到系统托盘", desc: "后台继续签到和同步" },
  { value: "exit", label: "直接退出", desc: "关闭窗口即结束程序" },
];

/** 设置页的可折叠卡片：默认收起，点标题展开。 */
function SetBlock({
  title,
  meta,
  action,
  open,
  onToggle,
  children,
}: {
  title: string;
  meta?: string;
  action?: ReactNode;
  open: boolean;
  onToggle: () => void;
  children: ReactNode;
}) {
  return (
    <div className={`set-block ${open ? "open" : ""}`}>
      <div className="set-block-head">
        <button
          type="button"
          className="set-toggle"
          aria-expanded={open}
          onClick={onToggle}
        >
          <span className="set-title">{title}</span>
          {meta && <span className="set-meta">{meta}</span>}
          <svg
            className="set-chevron"
            width="14"
            height="14"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
            aria-hidden
          >
            <path d="m6 9 6 6 6-6" />
          </svg>
        </button>
        {action}
      </div>
      {open && <div className="set-block-body">{children}</div>}
    </div>
  );
}

interface Props {
  overview: Overview;
  config: AppConfig;
  busy: boolean;
  syncing: boolean;
  onSchedulerEnabled: (enabled: boolean) => void;
  onSyncEnabled: (enabled: boolean) => void;
  onRefreshInterval: (providerId: string, minutes: number) => void;
  onScheduleChange: (
    providerId: string,
    patch: { enabled?: boolean; startMinute?: number; endMinute?: number }
  ) => void;
  onSyncNow: () => void;
  onOpenDataDir: () => void;
  onCloseAction: (action: CloseAction) => void;
  /** null = 状态还没读到 / 读取失败 */
  autostartEnabled: boolean | null;
  onAutostartChange: (enabled: boolean) => void;
  onNotificationsEnabled: (enabled: boolean) => void;
  onTestNotification: () => void;
}

/**
 * 设置页。
 *
 * 「自动同步」参考 Cockpit Tools 的 `*_auto_refresh_minutes`：按 agent 各自配间隔，
 * 后台到点就刷新积分 / 签到状态 / 账号资料。跨天时不看间隔，强制同步一次 ——
 * 这样新的一天开始后，「今日已签 / 今日可签」不会停在昨天。
 */
export default function SettingsPanel({
  overview,
  config,
  busy,
  syncing,
  onSchedulerEnabled,
  onSyncEnabled,
  onRefreshInterval,
  onScheduleChange,
  onSyncNow,
  onOpenDataDir,
  onCloseAction,
  autostartEnabled,
  onAutostartChange,
  onNotificationsEnabled,
  onTestNotification,
}: Props) {
  const providers = overview.providers;
  const agents = overview.agents;

  /** 设置页分 tab：「常规」放同步/调度/数据说明，「历史备份」放换号聊天记录的备份管理。 */
  const [tab, setTab] = useState<"general" | "backups">("general");
  /** 展开的卡片（默认全收起）。允许多张同时展开。 */
  const [openBlocks, setOpenBlocks] = useState<Set<string>>(() => new Set());
  const toggleBlock = (id: string) =>
    setOpenBlocks((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  const closeLabel =
    CLOSE_OPTIONS.find((o) => o.value === config.closeAction)?.label ?? "";

  const providerNames = useMemo(
    () => Object.fromEntries(providers.map((p) => [p.id, p.name])),
    [providers]
  );
  const providerAccents = useMemo(
    () => Object.fromEntries(providers.map((p) => [p.id, p.accent])),
    [providers]
  );

  return (
    <section className="panel settings">
      <header className="panel-head">
        <div className="panel-title">
          <span className="panel-mark settings-panel-mark">
            <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
              <circle cx="12" cy="12" r="3" />
              <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
            </svg>
          </span>
          <div>
            <h2>设置</h2>
            <p className="panel-sub">同步 / 调度 / 数据目录 / 历史备份</p>
          </div>
        </div>
        <div className="panel-actions">
          <button className="btn small" onClick={onSyncNow} disabled={busy || syncing}>
            {syncing ? "同步中…" : "立即同步"}
          </button>
          <button className="btn small" onClick={onOpenDataDir} disabled={busy}>
            打开数据目录
          </button>
        </div>
      </header>

      <div className="set-tabs" role="tablist">
        <button
          role="tab"
          aria-selected={tab === "general"}
          className={`set-tab ${tab === "general" ? "active" : ""}`}
          onClick={() => setTab("general")}
        >
          常规
        </button>
        <button
          role="tab"
          aria-selected={tab === "backups"}
          className={`set-tab ${tab === "backups" ? "active" : ""}`}
          onClick={() => setTab("backups")}
        >
          历史备份
        </button>
      </div>

      {tab === "backups" ? (
        <BackupPanel providerNames={providerNames} providerAccents={providerAccents} />
      ) : (
        <div className="set-grid">
        {/* ---------- 自动同步 ---------- */}
        <SetBlock
          title="自动同步"
          open={openBlocks.has("sync")}
          onToggle={() => toggleBlock("sync")}
          action={
            <label className="switch">
              <input
                type="checkbox"
                checked={config.scheduler.syncEnabled}
                onChange={(e) => onSyncEnabled(e.target.checked)}
              />
              <span>{config.scheduler.syncEnabled ? "已开启" : "已关闭"}</span>
            </label>
          }
        >
          <p className="set-hint">
            后台按下面的间隔刷新每个平台的积分、签到状态和账号资料。
            跨天时会无视间隔强制同步一次，保证「今日已签 / 今日可签」是新的一天的状态。
          </p>

          <table className="set-table">
            <thead>
              <tr>
                <th>平台</th>
                <th className="r">间隔（分钟）</th>
                <th className="r">账号</th>
              </tr>
            </thead>
            <tbody>
              {providers.map((p) => {
                const view = agents[p.id];
                return (
                  <tr key={p.id}>
                    <td>
                      <span
                        className="set-dot"
                        style={{ background: p.accent }}
                      />
                      {p.name}
                    </td>
                    <td className="r">
                      <input
                        type="number"
                        min={1}
                        max={1440}
                        value={view?.refreshIntervalMinutes ?? 10}
                        onChange={(e) => {
                          const v = Number(e.target.value);
                          if (Number.isFinite(v) && v >= 1 && v <= 1440) {
                            onRefreshInterval(p.id, Math.round(v));
                          }
                        }}
                      />
                    </td>
                    <td className="r num">{view?.accountCount ?? 0}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </SetBlock>

        {/* ---------- 自动签到调度 ---------- */}
        <SetBlock
          title="自动签到调度"
          open={openBlocks.has("schedule")}
          onToggle={() => toggleBlock("schedule")}
          action={
            <label className="switch">
              <input
                type="checkbox"
                checked={config.scheduler.enabled}
                onChange={(e) => onSchedulerEnabled(e.target.checked)}
              />
              <span>{config.scheduler.enabled ? "已开启" : "已关闭"}</span>
            </label>
          }
        >
          <p className="set-hint">
            总闸关掉后所有平台都不再自动签到。每个平台有自己独立的时间窗，
            窗内给每个账号随机分配一个错峰分钟。
          </p>

          <table className="set-table">
            <thead>
              <tr>
                <th>平台</th>
                <th className="c">启用</th>
                <th className="c">时间窗</th>
              </tr>
            </thead>
            <tbody>
              {providers.map((p) => {
                const view = agents[p.id];
                if (!view) return null;
                return (
                  <tr key={p.id}>
                    <td>
                      <span className="set-dot" style={{ background: p.accent }} />
                      {p.name}
                    </td>
                    <td className="c">
                      <input
                        type="checkbox"
                        checked={view.enabled}
                        onChange={(e) =>
                          onScheduleChange(p.id, { enabled: e.target.checked })
                        }
                      />
                    </td>
                    <td className="c">
                      <span className="set-win">
                        <TimeInput
                          value={view.startMinute}
                          onCommit={(m) => onScheduleChange(p.id, { startMinute: m })}
                          ariaLabel={`${p.name} 时间窗起`}
                        />
                        <span className="sched-dash">→</span>
                        <TimeInput
                          value={view.endMinute}
                          onCommit={(m) => onScheduleChange(p.id, { endMinute: m })}
                          ariaLabel={`${p.name} 时间窗止`}
                        />
                      </span>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </SetBlock>

        {/* ---------- 关闭窗口 ---------- */}
        <SetBlock
          title="关闭窗口"
          meta={closeLabel}
          open={openBlocks.has("close")}
          onToggle={() => toggleBlock("close")}
        >
          <p className="set-hint">
            点右上角 × 时的行为。最小化到托盘后，自动签到和后台同步会继续运行；
            要彻底结束程序，用托盘右键菜单里的「退出」。
          </p>
          <div className="close-choices">
            {CLOSE_OPTIONS.map((opt) => (
              <label
                key={opt.value}
                className={`close-choice ${config.closeAction === opt.value ? "active" : ""}`}
              >
                <input
                  type="radio"
                  name="close-action"
                  value={opt.value}
                  checked={config.closeAction === opt.value}
                  onChange={() => onCloseAction(opt.value)}
                />
                <span className="close-choice-text">
                  <span className="close-choice-label">{opt.label}</span>
                  <span className="close-choice-desc">{opt.desc}</span>
                </span>
              </label>
            ))}
          </div>
        </SetBlock>

        {/* ---------- 开机启动 ---------- */}
        <SetBlock
          title="开机启动"
          meta={autostartEnabled === null ? "读取中" : undefined}
          open={openBlocks.has("autostart")}
          onToggle={() => toggleBlock("autostart")}
          action={
            <label className="switch">
              <input
                type="checkbox"
                checked={autostartEnabled === true}
                disabled={autostartEnabled === null}
                onChange={(e) => onAutostartChange(e.target.checked)}
              />
              <span>
                {autostartEnabled === null
                  ? "读取中"
                  : autostartEnabled
                    ? "已开启"
                    : "已关闭"}
              </span>
            </label>
          }
        >
          <p className="set-hint">
            开机后静默启动到系统托盘，自动签到不用等你手动打开；
            启动时不弹窗、不抢焦点。要彻底结束程序，用托盘右键「退出」。
          </p>
        </SetBlock>

        {/* ---------- 应用内提醒 ---------- */}
        <SetBlock
          title="应用内提醒"
          open={openBlocks.has("notify")}
          onToggle={() => toggleBlock("notify")}
          action={
            <label className="switch">
              <input
                type="checkbox"
                checked={config.notificationsEnabled}
                onChange={(e) => onNotificationsEnabled(e.target.checked)}
              />
              <span>{config.notificationsEnabled ? "已开启" : "已关闭"}</span>
            </label>
          }
        >
          <p className="set-hint">
            自动签到失败、额度低于预警线、即将到期或可领取时会生成提醒。
            窗口隐藏时也会保存，可通过窗口顶部或托盘右键的「应用内提醒」查看。
          </p>
          <button
            className="btn small"
            style={{ alignSelf: "flex-start" }}
            onClick={onTestNotification}
          >
            发送测试提醒
          </button>
        </SetBlock>

        {/* ---------- 数据 ---------- */}
        <SetBlock
          title="数据"
          open={openBlocks.has("data")}
          onToggle={() => toggleBlock("data")}
        >
          <p className="set-hint">
            账号池、配置、调度计划和签到流水都存在这里。凭据用 Windows DPAPI 加密后落盘，
            明文只存在于内存，不会写进日志。
          </p>
          <p className="set-path">{overview.dataDir}</p>
        </SetBlock>

        {/* ---------- 说明 ---------- */}
        <SetBlock
          title="说明"
          open={openBlocks.has("about")}
          onToggle={() => toggleBlock("about")}
        >
          <p className="set-hint">
            本工具调用的是各客户端的非公开内部接口，仅用于把自己的多个账号聚合到一处管理，
            请遵守各平台的服务条款。接口随时可能变更，变更后这里的显示可能不准确。
          </p>
        </SetBlock>
        </div>
      )}
    </section>
  );
}
