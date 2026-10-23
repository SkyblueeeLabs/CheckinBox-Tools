import type { AppConfig, Overview } from "../lib/types";
import { formatMinute } from "../lib/api";

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
}: Props) {
  const providers = overview.providers;
  const agents = overview.agents;

  return (
    <section className="panel settings">
      <header className="panel-head">
        <div className="panel-title">
          <span className="panel-mark" />
          <div>
            <h2>设置</h2>
            <p className="panel-sub">同步 / 调度 / 数据目录</p>
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

      <div className="set-grid">
        {/* ---------- 自动同步 ---------- */}
        <div className="set-block">
          <div className="set-block-head">
            <h3>自动同步</h3>
            <label className="switch">
              <input
                type="checkbox"
                checked={config.scheduler.syncEnabled}
                onChange={(e) => onSyncEnabled(e.target.checked)}
              />
              <span>{config.scheduler.syncEnabled ? "已开启" : "已关闭"}</span>
            </label>
          </div>
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
        </div>

        {/* ---------- 自动签到调度 ---------- */}
        <div className="set-block">
          <div className="set-block-head">
            <h3>自动签到调度</h3>
            <label className="switch">
              <input
                type="checkbox"
                checked={config.scheduler.enabled}
                onChange={(e) => onSchedulerEnabled(e.target.checked)}
              />
              <span>{config.scheduler.enabled ? "已开启" : "已关闭"}</span>
            </label>
          </div>
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
                        <input
                          value={formatMinute(view.startMinute)}
                          onChange={(e) => {
                            const text = e.target.value;
                            const parts = text.split(":").map(Number);
                            if (parts.length !== 2 || parts.some((n) => !Number.isFinite(n))) return;
                            const m = parts[0] * 60 + parts[1];
                            if (m >= 0 && m < 24 * 60) {
                              onScheduleChange(p.id, { startMinute: m });
                            }
                          }}
                        />
                        <span className="sched-dash">→</span>
                        <input
                          value={formatMinute(view.endMinute)}
                          onChange={(e) => {
                            const text = e.target.value;
                            const parts = text.split(":").map(Number);
                            if (parts.length !== 2 || parts.some((n) => !Number.isFinite(n))) return;
                            const m = parts[0] * 60 + parts[1];
                            if (m > 0 && m <= 24 * 60) {
                              onScheduleChange(p.id, { endMinute: m });
                            }
                          }}
                        />
                      </span>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>

        {/* ---------- 数据 ---------- */}
        <div className="set-block">
          <div className="set-block-head">
            <h3>数据</h3>
          </div>
          <p className="set-hint">
            账号池、配置、调度计划和签到流水都存在这里。凭据用 Windows DPAPI 加密后落盘，
            明文只存在于内存，不会写进日志。
          </p>
          <p className="set-path">{overview.dataDir}</p>
        </div>

        {/* ---------- 说明 ---------- */}
        <div className="set-block">
          <div className="set-block-head">
            <h3>说明</h3>
          </div>
          <p className="set-hint">
            本工具调用的是各客户端的非公开内部接口，仅用于把自己的多个账号聚合到一处管理，
            请遵守各平台的服务条款。接口随时可能变更，变更后这里的显示可能不准确。
          </p>
        </div>
      </div>
    </section>
  );
}
