import { useCallback, useEffect, useRef, useState } from "react";
import { api, onCheckinProgress } from "./lib/api";
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

/** 把 overview 折成侧边栏/面板要用的分组 */
function buildGroups(overview: Overview): AgentGroup[] {
  return overview.providers.map((provider) => {
    const accounts = overview.accounts.filter(
      (a) => a.account.provider === provider.id
    );
    return {
      provider,
      accounts,
      totalRemaining: accounts.reduce((s, a) => s + (a.quota?.totalRemaining ?? 0), 0),
      totalGranted: accounts.reduce((s, a) => s + (a.quota?.totalGranted ?? 0), 0),
      claimableCount: accounts.filter((a) => a.status?.state === "claimable").length,
      claimedToday: accounts.filter((a) => a.status?.state === "claimed").length,
      schedule: overview.agents[provider.id] ?? {
        enabled: true,
        startMinute: 6 * 60,
        endMinute: 8 * 60,
        accountCount: accounts.length,
        todayClaimed: 0,
        todayClaimable: 0,
      },
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
  const [authUrl, setAuthUrl] = useState<string | null>(null);
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
        prev && ov.providers.some((p) => p.id === prev)
          ? prev
          : ov.providers[0]?.id ?? ""
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
          `${ok} 成功 / ${already} 已签，${failed.length} 个失败：${failed
            .map((f) => `${f.label}（${f.message}）`)
            .join("；")}`
        );
      } else {
        pushToast("ok", `${ok} 个签到成功，${already} 个今日已签`);
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
      if (result.status === "success") pushToast("ok", `${label} +${result.points}`);
      else if (result.status === "already") pushToast("warn", `${label} 今日已签`);
      else pushToast("warn", `${label}：${result.message}`);
    } catch (e) {
      pushToast("err", `${label}：${e}`);
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
      pushToast("ok", `已导入：${hit.account.label}`);
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
      setAuthUrl(challenge.authUrl);
      await api.openExternal(challenge.authUrl);
      pushToast("warn", "请在浏览器里确认授权");
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

  // 应用重启后恢复未完成的授权轮询
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const pending = await api.pendingAuth();
        if (!cancelled && pending.length > 0) {
          setAuthSession(pending[0].sessionId);
          setAuthUrl(pending[0].authUrl);
        }
      } catch {
        /* 忽略 */
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const removeAccount = async (providerId: string, accountId: string, label: string) => {
    try {
      await api.removeAccount(providerId, accountId);
      pushToast("warn", `已移除：${label}`);
      await refresh();
    } catch (e) {
      pushToast("err", `移除失败：${e}`);
    }
  };

  const setActive = async (providerId: string, accountId: string) => {
    try {
      await api.setActiveAccount(providerId, accountId);
      pushToast("ok", "已设为当前账号");
      await refresh();
    } catch (e) {
      pushToast("err", `切换失败：${e}`);
    }
  };

  const launch = async (providerId: string) => {
    try {
      const exe = await api.launchClient(providerId);
      pushToast("ok", `已打开客户端`);
      void exe;
    } catch (e) {
      pushToast("err", `${e}`);
    }
  };

  /** 改某个 agent 自己的自动签到配置（前端乐观更新 + 后端落盘） */
  const patchSchedule = useCallback(
    (providerId: string, patch: { enabled?: boolean; startMinute?: number; endMinute?: number }) => {
      setOverview((prev) => {
        if (!prev) return prev;
        const cur = prev.agents[providerId];
        if (!cur) return prev;
        return {
          ...prev,
          agents: {
            ...prev.agents,
            [providerId]: { ...cur, ...patch },
          },
        };
      });
      void api
        .setAgentSchedule(
          providerId,
          patch.enabled ?? overview?.agents[providerId]?.enabled ?? true,
          patch.startMinute ?? overview?.agents[providerId]?.startMinute ?? 360,
          patch.endMinute ?? overview?.agents[providerId]?.endMinute ?? 480
        )
        .then((cfg) => setConfig(cfg))
        .catch((e) => pushToast("err", `保存失败：${e}`));
    },
    [overview, pushToast]
  );

  const toggleMaster = async (enabled: boolean) => {
    try {
      const cfg = await api.setSchedulerEnabled(enabled);
      setConfig(cfg);
    } catch (e) {
      pushToast("err", `${e}`);
    }
  };

  if (!overview || !config) {
    return (
      <div className="loading">
        <div className="loading-mark" />
        <p>加载中…</p>
      </div>
    );
  }

  const groups = buildGroups(overview);
  const current = groups.find((g) => g.provider.id === selected) ?? groups[0];

  return (
    <div className="shell">
      <Sidebar
        groups={groups}
        selected={current?.provider.id ?? ""}
        onSelect={setSelected}
      />

      <div className="main">
        {authSession && authUrl && (
          <div className="auth-banner">
            <div className="auth-text">
              <strong>等待 Qoder 授权</strong>
              <span>
                已在浏览器打开授权页。
                <button className="link" onClick={() => api.openExternal(authUrl)}>
                  重新打开
                </button>
                用要登录的账号点「确认授权」。
              </span>
            </div>
            <button className="btn small" onClick={() => setAuthSession(null)}>
              取消
            </button>
          </div>
        )}

        <header className="topbar">
          <div className="brand">
            <span className="brand-name">聚合签到</span>
            <span className="brand-sub">
              今日已签 <b>{overview.claimedCount}</b> / {overview.totalCount} 个账号
            </span>
          </div>

          <div className="topbar-right">
            <label className="switch" title="总闸：关掉后所有 agent 都不跑">
              <input
                type="checkbox"
                checked={config.scheduler.enabled}
                onChange={(e) => toggleMaster(e.target.checked)}
              />
              <span>总闸</span>
            </label>
            <button className="btn primary" onClick={runAll} disabled={busy}>
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
            onSetActive={setActive}
            onLaunch={launch}
            onRemove={removeAccount}
            onDiscover={() => discover(current.provider.id)}
            onAddAccount={startQoderAuth}
            onScheduleChange={(id, patch) => patchSchedule(id, patch)}
          />
        ) : (
          <div className="empty">
            <p>还没有任何 agent 分组。先在左侧选择一个平台添加账号。</p>
          </div>
        )}

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
