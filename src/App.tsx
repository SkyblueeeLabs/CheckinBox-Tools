import { useCallback, useEffect, useRef, useState } from "react";
import {
  api,
  fmtPoints,
  formatMinute,
  onCheckinProgress,
  parseMinute,
} from "./lib/api";
import type {
  AccountResult,
  AgentGroup,
  AppConfig,
  LogEntry,
  Overview,
} from "./lib/types";
import Sidebar from "./components/Sidebar";
import AgentGroupPanel from "./components/AgentGroupPanel";
import LogPanel from "./components/LogPanel";

interface Toast {
  id: number;
  kind: "ok" | "warn" | "err";
  text: string;
}

/** 把 overview 折成侧边栏要用的分组摘要 */
function buildGroups(overview: Overview): AgentGroup[] {
  return overview.providers.map((provider) => {
    const accounts = overview.accounts.filter(
      (a) => a.account.provider === provider.id
    );
    return {
      provider,
      accounts,
      totalRemaining: accounts.reduce(
        (sum, a) => sum + (a.quota?.totalRemaining ?? 0),
        0
      ),
      totalGranted: accounts.reduce(
        (sum, a) => sum + (a.quota?.totalGranted ?? 0),
        0
      ),
      claimableCount: accounts.filter(
        (a) => a.status?.state === "claimable"
      ).length,
      claimedToday: accounts.filter(
        (a) => a.status?.state === "claimed"
      ).length,
    };
  });
}

export default function App() {
  const [overview, setOverview] = useState<Overview | null>(null);
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [selected, setSelected] = useState<string>("");
  const [busy, setBusy] = useState(false);
  const [quotaLoadingId, setQuotaLoadingId] = useState<string | null>(null);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [authSession, setAuthSession] = useState<string | null>(null);
  const [logsOpen, setLogsOpen] = useState(true);
  const toastSeq = useRef(0);

  const pushToast = useCallback((kind: Toast["kind"], text: string) => {
    const id = ++toastSeq.current;
    setToasts((prev) => [...prev, { id, kind, text }]);
    setTimeout(() => setToasts((prev) => prev.filter((t) => t.id !== id)), 5200);
  }, []);

  const refresh = useCallback(async () => {
    try {
      const [ov, lg, cfg] = await Promise.all([
        api.overview(),
        api.getLogs(7),
        api.getConfig(),
      ]);
      setOverview(ov);
      setLogs(lg);
      setConfig(cfg);
      setSelected((prev) =>
        prev && ov.providers.some((p) => p.id === prev) ? prev : ov.providers[0]?.id ?? ""
      );
    } catch (e) {
      pushToast("err", `加载失败：${e}`);
    }
  }, [pushToast]);

  useEffect(() => {
    void refresh();
    const unlisten = onCheckinProgress((result: AccountResult) => {
      if (result.status === "success") {
        pushToast("ok", `${result.label} 签到成功 +${result.points}`);
      } else if (result.status === "failed") {
        pushToast("err", `${result.label} 签到失败：${result.message}`);
      }
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [refresh, pushToast]);

  // 后台定时刷新（积分和签到状态都会变）
  useEffect(() => {
    const timer = setInterval(() => void refresh(), 45_000);
    return () => clearInterval(timer);
  }, [refresh]);

  const runAll = async () => {
    setBusy(true);
    try {
      const results = await api.checkinAll();
      const ok = results.filter((r) => r.status === "success").length;
      const already = results.filter((r) => r.status === "already").length;
      const failed = results.filter((r) => r.status === "failed");
      if (failed.length) {
        pushToast(
          "warn",
          `完成 ${ok} 成功 / ${already} 已签，${failed.length} 个失败：${failed
            .map((f) => `${f.label}（${f.message}）`)
            .join("；")}`
        );
      } else {
        pushToast("ok", `完成：${ok} 个签到成功，${already} 个今日已签`);
      }
    } catch (e) {
      pushToast("err", `签到失败：${e}`);
    } finally {
      setBusy(false);
      await refresh();
    }
  };

  const runOne = async (providerId: string, accountId: string, label: string) => {
    setBusy(true);
    try {
      const result = await api.checkinAccount(providerId, accountId);
      if (result.status === "success") {
        pushToast("ok", `${label} 签到成功 +${result.points}`);
      } else if (result.status === "already") {
        pushToast("warn", `${label} 今日已签到`);
      } else {
        pushToast("warn", `${label}：${result.message}`);
      }
    } catch (e) {
      pushToast("err", `${label} 签到失败：${e}`);
    } finally {
      setBusy(false);
      await refresh();
    }
  };

  const refreshOneQuota = async (providerId: string, accountId: string) => {
    setQuotaLoadingId(`${providerId}:${accountId}`);
    try {
      await api.refreshQuota(providerId, accountId);
      await refresh();
    } catch (e) {
      pushToast("err", `积分刷新失败：${e}`);
    } finally {
      setQuotaLoadingId(null);
    }
  };

  const discover = async (providerId: string) => {
    setBusy(true);
    try {
      const found = await api.discoverAccounts();
      const hit = found.find((f) => f.provider === providerId);
      if (!hit) {
        pushToast("warn", "没有找到本机客户端登录态");
        return;
      }
      await api.addDiscoveredAccount(providerId);
      pushToast("ok", `已导入账号：${hit.account.label}`);
    } catch (e) {
      pushToast("err", `导入失败：${e}`);
    } finally {
      setBusy(false);
      await refresh();
    }
  };

  const startQoderAuth = async () => {
    try {
      const challenge = await api.beginQoderAuth();
      setAuthSession(challenge.sessionId);
      await api.openExternal(challenge.authUrl);
      pushToast("warn", "请在浏览器里用要登录的账号确认授权");
    } catch (e) {
      pushToast("err", `发起授权失败：${e}`);
    }
  };

  // 轮询 Qoder 设备授权结果
  useEffect(() => {
    if (!authSession) return;
    const timer = setInterval(async () => {
      try {
        const account = await api.pollQoderAuth(authSession);
        if (account) {
          setAuthSession(null);
          pushToast("ok", `已添加账号：${account.label}`);
          await refresh();
        }
      } catch (e) {
        setAuthSession(null);
        pushToast("err", `授权失败：${e}`);
      }
    }, 2500);
    return () => clearInterval(timer);
  }, [authSession, pushToast, refresh]);

  const removeAccount = async (providerId: string, accountId: string, label: string) => {
    try {
      await api.removeAccount(providerId, accountId);
      pushToast("warn", `已移除账号：${label}`);
      await refresh();
    } catch (e) {
      pushToast("err", `移除失败：${e}`);
    }
  };

  const updateConfig = async (next: AppConfig) => {
    setConfig(next);
    try {
      await api.setConfig(next);
    } catch (e) {
      pushToast("err", `保存配置失败：${e}`);
      await refresh();
    }
  };

  if (!overview || !config) {
    return (
      <div className="loading">
        <div className="loading-logo" />
        <p>加载中…</p>
      </div>
    );
  }

  const groups = buildGroups(overview);
  const current = groups.find((g) => g.provider.id === selected) ?? groups[0];
  const totalRemaining = groups.reduce((s, g) => s + g.totalRemaining, 0);

  return (
    <div className="shell">
      <Sidebar
        groups={groups}
        selected={current?.provider.id ?? ""}
        activeCount={overview.totalCount}
        onSelect={setSelected}
      />

      <div className="main">
        <header className="topbar">
          <div className="topbar-left">
            <h1>聚合签到</h1>
            <p className="topbar-sub">
              今日已签 {overview.claimedCount}/{overview.totalCount} 个账号 · 累计发放 +
              {fmtPoints(
                groups.reduce((s, g) => s + g.totalGranted, 0)
              )}
            </p>
          </div>

          <div className="topbar-right">
            <div className="topbar-quota">
              <span className="topbar-quota-num">{fmtPoints(totalRemaining)}</span>
              <span className="topbar-quota-key">总剩余积分</span>
            </div>
            <button className="primary" onClick={runAll} disabled={busy}>
              {busy ? "签到中…" : "一键全部签到"}
            </button>
          </div>
        </header>

        {current ? (
          <AgentGroupPanel
            group={current}
            busy={busy}
            quotaLoadingId={quotaLoadingId}
            authPending={authSession !== null}
            onCheckin={runOne}
            onRefreshQuota={refreshOneQuota}
            onDiscover={() => discover(current.provider.id)}
            onAddAccount={startQoderAuth}
            onRemove={removeAccount}
          />
        ) : (
          <div className="group-empty">
            <p>还没有任何 agent 分组。先在左侧选择一个平台添加账号。</p>
          </div>
        )}

        <section className="scheduler">
          <label className="switch">
            <input
              type="checkbox"
              checked={config.scheduler.enabled}
              onChange={(e) =>
                updateConfig({
                  ...config,
                  scheduler: { ...config.scheduler, enabled: e.target.checked },
                })
              }
            />
            <span>自动签到</span>
          </label>

          <div className="window">
            <span className="window-label">时间窗</span>
            <input
              className="time"
              value={formatMinute(config.scheduler.startMinute)}
              onChange={(e) => {
                const m = parseMinute(e.target.value);
                if (m !== null) {
                  updateConfig({
                    ...config,
                    scheduler: { ...config.scheduler, startMinute: m },
                  });
                }
              }}
            />
            <span className="window-dash">→</span>
            <input
              className="time"
              value={formatMinute(config.scheduler.endMinute)}
              onChange={(e) => {
                const m = parseMinute(e.target.value);
                if (m !== null) {
                  updateConfig({
                    ...config,
                    scheduler: { ...config.scheduler, endMinute: m },
                  });
                }
              }}
            />
            <span className="window-hint">窗内每账号随机错峰执行</span>
          </div>

          <div className="provider-toggles">
            {overview.providers.map((p) => (
              <label key={p.id} className="chip">
                <input
                  type="checkbox"
                  checked={config.scheduler.providers[p.id] ?? true}
                  onChange={(e) =>
                    updateConfig({
                      ...config,
                      scheduler: {
                        ...config.scheduler,
                        providers: {
                          ...config.scheduler.providers,
                          [p.id]: e.target.checked,
                        },
                      },
                    })
                  }
                />
                <span className="chip-dot" style={{ background: p.accent }} />
                {p.name}
              </label>
            ))}
          </div>
        </section>

        <LogPanel logs={logs} visible={logsOpen} onToggle={() => setLogsOpen((v) => !v)} />

        <div className="toasts">
          {toasts.map((t) => (
            <div key={t.id} className={`toast ${t.kind}`}>
              {t.text}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
