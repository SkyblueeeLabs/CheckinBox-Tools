import { useCallback, useEffect, useRef, useState } from "react";
import { api, onAccountsSynced, onCheckinProgress, onCloseRequested } from "./lib/api";
import { buildGroups } from "./lib/groups";
import type {
  AccountResult,
  AppConfig,
  CloseAction,
  LogEntry,
  Overview,
} from "./lib/types";
import Sidebar, { SETTINGS_ID } from "./components/Sidebar";
import AgentGroupPanel from "./components/AgentGroupPanel";
import DashboardPanel from "./components/DashboardPanel";
import SettingsPanel from "./components/SettingsPanel";
import LogPanel from "./components/LogPanel";
import CloseConfirmDialog from "./components/CloseConfirmDialog";
import { HOME_ID } from "./lib/types";

interface Toast {
  id: number;
  kind: "ok" | "warn" | "err";
  text: string;
}

export default function App() {
  const [overview, setOverview] = useState<Overview | null>(null);
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [selected, setSelected] = useState<string>(HOME_ID);
  const [busy, setBusy] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const [quotaLoadingId, setQuotaLoadingId] = useState<string | null>(null);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [authSession, setAuthSession] = useState<string | null>(null);
  const [authUrl, setAuthUrl] = useState<string | null>(null);
  const [authProvider, setAuthProvider] = useState<string | null>(null);
  const [logsOpen, setLogsOpen] = useState(false);
  const [closePrompt, setClosePrompt] = useState(false);
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
        prev === SETTINGS_ID || prev === HOME_ID || ov.providers.some((p) => p.id === prev)
          ? prev
          : HOME_ID
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
    // 后台自动同步跑完一轮，界面跟着刷新（跨天同步尤其重要）
    const unlistenSync = onAccountsSynced(() => {
      void refresh();
    });
    // 点了窗口 ×、关闭行为是「每次询问」时，后端会拦下关闭并通知这里
    const unlistenClose = onCloseRequested(() => setClosePrompt(true));
    return () => {
      void unlisten.then((fn) => fn());
      void unlistenSync.then((fn) => fn());
      void unlistenClose.then((fn) => fn());
    };
  }, [refresh, pushToast]);

  // 前台轮询（积分和签到状态都会变）
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
      const found = (await api.discoverAccounts()).filter((f) => f.provider === providerId);
      if (found.length === 0) {
        pushToast("warn", "没有找到本机客户端登录态");
        return;
      }
      await api.addDiscoveredAccount(providerId);
      // 已在池里的账号这次不会重复入库；一个新账号都没有时，
      // 大概率是「客户端登录的还是老号」—— 直接告诉用户该怎么做
      const fresh = found.filter((f) => !f.alreadyAdded);
      if (fresh.length > 0) {
        pushToast("ok", `已导入 ${fresh.length} 个账号：${fresh.map((f) => f.account.label).join("、")}`);
      } else {
        pushToast(
          "warn",
          `客户端当前登录的是「${found[0].account.label}」，已在账号池里。要导入新账号，请先在客户端登录它，再点「从本机导入」`
        );
      }
    } catch (e) {
      pushToast("err", `导入失败：${e}`);
    } finally {
      setBusy(false);
      await refresh();
    }
  };

  const startDeviceAuth = async (providerId: string) => {
    try {
      const challenge = await api.beginDeviceAuth(providerId);
      setAuthSession(challenge.sessionId);
      setAuthProvider(providerId);
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
        const account = await api.pollDeviceAuth(authProvider ?? "qoder", authSession);
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
          // 待完成会话自带 provider（命令行入驻时可能直接指定）
          setAuthProvider(pending[0].provider ?? "qoder");
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

  /**
   * 把客户端切换成这个账号：**关闭客户端 → 备份登录态 → 写入这个账号的凭据 →
   * 以它重启客户端**，一条龙（借鉴 Cockpit 的 launch_on_switch）。
   * 客户端在运行也没关系，后端会先自动收掉它；当前登录的账号切走前会被捕获进池。
   */
  const switchClient = async (providerId: string, accountId: string) => {
    const view = overview?.accounts.find(
      (a) => a.account.provider === providerId && a.account.id === accountId
    );
    const label = view?.account.label ?? accountId;
    setBusy(true);
    try {
      const result = await api.switchClientAccount(providerId, accountId);
      const chatNote = result.chatSync ? `；${result.chatSync.detail}` : "";
      pushToast(
        "ok",
        `${label}：已切换客户端账号并重启，同时设为当前账号${result.backup ? "（原登录态已备份）" : ""}${chatNote}`
      );
      await refresh();
    } catch (e) {
      pushToast("err", `切换失败：${e}`);
    } finally {
      setBusy(false);
    }
  };

  /** 改某个 agent 的「同步聊天记录」开关（乐观更新 + 后端落盘，重启后保持） */
  const patchChatSync = useCallback(
    (providerId: string, enabled: boolean) => {
      setOverview((prev) => {
        if (!prev) return prev;
        const cur = prev.agents[providerId];
        if (!cur) return prev;
        return {
          ...prev,
          agents: {
            ...prev.agents,
            [providerId]: { ...cur, chatSyncEnabled: enabled },
          },
        };
      });
      void api
        .setChatSync(providerId, enabled)
        .then((cfg) => setConfig(cfg))
        .catch((e) => pushToast("err", `保存失败：${e}`));
    },
    [pushToast]
  );

  /** 改某个 agent 的积分预警阈值 */
  const patchAlert = useCallback((providerId: string, value: number) => {
    setOverview((prev) => {
      if (!prev) return prev;
      return {
        ...prev,
        accounts: prev.accounts.map((a) =>
          a.account.provider === providerId ? { ...a, isLow: (a.quota?.totalRemaining ?? 0) < value, alertThreshold: value } : a
        ),
      };
    });
    void api
      .setAlertThreshold(providerId, value)
      .then((cfg) => setConfig(cfg))
      .catch((e) => pushToast("err", `保存失败：${e}`));
  }, [pushToast]);

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

  const patchRefreshInterval = useCallback(
    (providerId: string, minutes: number) => {
      setOverview((prev) => {
        if (!prev) return prev;
        const cur = prev.agents[providerId];
        if (!cur) return prev;
        return {
          ...prev,
          agents: {
            ...prev.agents,
            [providerId]: { ...cur, refreshIntervalMinutes: minutes },
          },
        };
      });
      void api
        .setRefreshInterval(providerId, minutes)
        .then((cfg) => setConfig(cfg))
        .catch((e) => pushToast("err", `保存失败：${e}`));
    },
    [pushToast]
  );

  const toggleMaster = async (enabled: boolean) => {
    try {
      const cfg = await api.setSchedulerEnabled(enabled);
      setConfig(cfg);
    } catch (e) {
      pushToast("err", `${e}`);
    }
  };

  const toggleSync = async (enabled: boolean) => {
    try {
      const cfg = await api.setSyncEnabled(enabled);
      setConfig(cfg);
      pushToast("ok", enabled ? "自动同步已开启" : "自动同步已关闭");
    } catch (e) {
      pushToast("err", `${e}`);
    }
  };

  const syncNow = async () => {
    setSyncing(true);
    try {
      const count = await api.syncNow();
      pushToast("ok", `已同步 ${count} 个账号`);
      await refresh();
    } catch (e) {
      pushToast("err", `同步失败：${e}`);
    } finally {
      setSyncing(false);
    }
  };

  /** 设置页里改「关闭窗口」行为（落盘，重启后保持） */
  const changeCloseAction = useCallback(
    async (action: CloseAction) => {
      try {
        const cfg = await api.setCloseAction(action);
        setConfig(cfg);
      } catch (e) {
        pushToast("err", `保存失败：${e}`);
      }
    },
    [pushToast]
  );

  /** 关闭确认框的选择结果：执行一次；勾了「记住」才写进配置 */
  const resolveClose = useCallback(
    async (action: "tray" | "exit", remember: boolean) => {
      setClosePrompt(false);
      try {
        await api.resolveCloseAction(action, remember);
        if (remember) {
          setConfig((prev) => (prev ? { ...prev, closeAction: action } : prev));
        }
      } catch (e) {
        pushToast("err", `关闭失败：${e}`);
      }
    },
    [pushToast]
  );

  const openDataDir = async () => {
    try {
      await api.openDataDir();
    } catch (e) {
      pushToast("err", `${e}`);
    }
  };

  // 首屏骨架：不等 overview（它要等各平台网络请求，3 秒级）。
  // 立即渲染界面结构 + 骨架占位，数据到了自动填充（45s 轮询 / 事件驱动）。
  if (!overview || !config) {
    return (
      <div className="shell">
        <aside className="sidebar">
          <div className="side-item side-home active">
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
          </div>
          <div className="sidebar-head">
            <span className="sidebar-title">Agents</span>
          </div>
          <nav className="sidebar-list">
            {[0, 1, 2, 3].map((i) => (
              <div key={i} className="side-item skeleton-item">
                <span className="skeleton skeleton-mark" />
                <span className="side-body">
                  <span className="skeleton skeleton-line" />
                  <span className="skeleton skeleton-line short" />
                </span>
              </div>
            ))}
          </nav>
        </aside>
        <div className="main">
          <header className="topbar">
            <div className="brand">
              <span className="brand-name">{selected === SETTINGS_ID ? "设置" : selected === HOME_ID ? "总览" : "CheckinBox"}</span>
            </div>
          </header>
          <section className="panel dash">
            <div className="dash-stats">
              {[0, 1, 2, 3].map((i) => (
                <div key={i} className="dash-stat">
                  <span className="skeleton skeleton-line" />
                  <span className="skeleton skeleton-value" />
                </div>
              ))}
            </div>
            <div className="dash-agents">
              {[0, 1, 2, 3].map((i) => (
                <div key={i} className="dash-agent skeleton-item">
                  <span className="skeleton skeleton-mark" />
                  <span className="skeleton skeleton-line" />
                  <span className="skeleton skeleton-line short" />
                </div>
              ))}
            </div>
          </section>
          <div className="loading-hint">
            <div className="loading-mark" />
            <p>正在加载账号与积分…</p>
          </div>
        </div>
      </div>
    );
  }

  const groups = buildGroups(overview);
  const inSettings = selected === SETTINGS_ID;
  const inHome = selected === HOME_ID;
  const current = groups.find((g) => g.provider.id === selected);
  const providerNames = Object.fromEntries(
    overview.providers.map((p) => [p.id, p.name])
  );
  // 账号 id → 当前名字：签到记录按 id 解析「现在叫什么」，
  // 不用写入时的快照（账号改名/换人用旧名后，历史名字会误导）
  const accountNames = Object.fromEntries(
    overview.accounts.map((a) => [`${a.account.provider}:${a.account.id}`, a.account.label])
  );

  return (
    <div className="shell">
      <Sidebar
        groups={groups}
        selected={selected}
        onSelect={setSelected}
        onOpenHome={() => setSelected(HOME_ID)}
        onOpenSettings={() => setSelected(SETTINGS_ID)}
        onReorder={(order) => {
          // 乐观更新本地顺序，落盘失败再回滚刷新
          setOverview((prev) =>
            prev
              ? {
                  ...prev,
                  providers: order
                    .map((id) => prev.providers.find((p) => p.id === id))
                    .filter((p): p is NonNullable<typeof p> => !!p),
                }
              : prev
          );
          void api
            .setAgentOrder(order)
            .then((cfg) => setConfig(cfg))
            .catch((e) => {
              pushToast("err", `保存顺序失败：${e}`);
              void refresh();
            });
        }}
      />

      <div className="main">
        {authSession && authUrl && (
          <div className="auth-banner">
            <div className="auth-text">
              <strong>等待 {providerNames[authProvider ?? ""] ?? "平台"} 授权</strong>
              <span>
                已在浏览器打开授权页。
                <button className="link" onClick={() => api.openExternal(authUrl)}>
                  重新打开
                </button>
                用要登录的账号点「确认授权」。
              </span>
            </div>
            <button className="btn small" onClick={() => { setAuthSession(null); setAuthProvider(null); }}>
              取消
            </button>
          </div>
        )}

        <header className="topbar">
          <div className="brand">
            <span className="brand-name">{inHome ? "总览" : inSettings ? "设置" : "CheckinBox"}</span>
            <span className="brand-sub">
              今日已签 <b>{overview.claimedCount}</b> / {overview.totalCount} 个账号
              {overview.todayPoints > 0 ? ` · 今日 +${overview.todayPoints}` : ""}
            </span>
          </div>

          <div className="topbar-right">
            <label className="switch" title="总闸：关掉后所有 agent 都不跑">
              <input
                type="checkbox"
                checked={config.scheduler.enabled}
                onChange={(e) => toggleMaster(e.target.checked)}
              />
              <span>签到总闸</span>
            </label>
            <label className="switch" title="后台自动同步积分 / 签到状态 / 账号资料">
              <input
                type="checkbox"
                checked={config.scheduler.syncEnabled}
                onChange={(e) => toggleSync(e.target.checked)}
              />
              <span>自动同步</span>
            </label>
            <button className="btn primary" onClick={runAll} disabled={busy}>
              {busy ? "签到中…" : "一键全部签到"}
            </button>
          </div>
        </header>

        {inHome ? (
          <DashboardPanel
            overview={overview}
            groups={groups}
            onOpenAgent={(id) => setSelected(id)}
          />
        ) : inSettings ? (
          <SettingsPanel
            overview={overview}
            config={config}
            busy={busy}
            syncing={syncing}
            onSchedulerEnabled={toggleMaster}
            onSyncEnabled={toggleSync}
            onRefreshInterval={patchRefreshInterval}
            onScheduleChange={(id, patch) => patchSchedule(id, patch)}
            onSyncNow={syncNow}
            onOpenDataDir={openDataDir}
            onCloseAction={(action) => void changeCloseAction(action)}
          />
        ) : current ? (
          <AgentGroupPanel
            group={current}
            busy={busy}
            quotaLoadingId={quotaLoadingId}
            authPending={authSession !== null}
            onCheckin={runOne}
            onRefreshQuota={refreshOneQuota}
            onSetActive={setActive}
            onLaunch={launch}
            onSwitchClient={switchClient}
            onRemove={removeAccount}
            onDiscover={(id) => discover(id)}
            onAddAccount={(id: string) => { void startDeviceAuth(id); }}
            onScheduleChange={(id, patch) => patchSchedule(id, patch)}
            onAlertChange={patchAlert}
            onChatSyncChange={patchChatSync}
          />
        ) : (
          <div className="empty-state">
            <p>还没有任何 agent 分组。先在左侧选择一个平台添加账号。</p>
          </div>
        )}

        <LogPanel
          logs={logs}
          providerId={current?.provider.id ?? null}
          providerName={current?.provider.name ?? "全部"}
          providerNames={providerNames}
          accountNames={accountNames}
          visible={logsOpen}
          onToggle={() => setLogsOpen((v) => !v)}
        />

        <div className="toasts">
          {toasts.map((t) => (
            <div key={t.id} className={`toast ${t.kind}`}>
              {t.text}
            </div>
          ))}
        </div>

        {closePrompt && (
          <CloseConfirmDialog
            onCancel={() => setClosePrompt(false)}
            onResolve={(action, remember) => void resolveClose(action, remember)}
          />
        )}
      </div>
    </div>
  );
}
