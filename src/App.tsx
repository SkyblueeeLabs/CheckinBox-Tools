import { useCallback, useEffect, useRef, useState } from "react";
import { api, onAccountsChanged, onNoticesChanged, onSwitchProgress, onWindowRequested } from "./lib/api";
import { buildGroups } from "./lib/groups";
import type {
  AgentVisibility,
  AppConfig,
  CloseAction,
  LiveFreeModels,
  LogEntry,
  Notice,
  NoticeKind,
  Overview,
  SwitchProgressStage,
} from "./lib/types";
import Sidebar, { SETTINGS_ID } from "./components/Sidebar";
import AgentGroupPanel from "./components/AgentGroupPanel";
import DashboardPanel from "./components/DashboardPanel";
import LaunchScreen from "./components/LaunchScreen";
import SettingsPanel from "./components/SettingsPanel";
import LogPanel from "./components/LogPanel";
import CloseConfirmDialog from "./components/CloseConfirmDialog";
import NotificationsDialog from "./components/NotificationsDialog";
import SwitchProgressDialog from "./components/SwitchProgressDialog";
import { HOME_ID } from "./lib/types";

interface Toast {
  id: number;
  kind: NoticeKind;
  text: string;
}

interface SwitchProgressState {
  providerId: string;
  providerName: string;
  accountId: string;
  accountLabel: string;
  chatSyncEnabled: boolean;
  stage: SwitchProgressStage | "preparing" | "completed" | "failed";
  detail?: string;
  error?: string;
}

export default function App() {
  const [overview, setOverview] = useState<Overview | null>(null);
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [autostart, setAutostart] = useState<boolean | null>(null);
  const [selected, setSelected] = useState<string>(HOME_ID);
  const [busy, setBusy] = useState(false);
  const [switchProgress, setSwitchProgress] = useState<SwitchProgressState | null>(null);
  const [syncing, setSyncing] = useState(false);
  const [quotaLoadingId, setQuotaLoadingId] = useState<string | null>(null);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [authSession, setAuthSession] = useState<string | null>(null);
  const [authUrl, setAuthUrl] = useState<string | null>(null);
  const [authProvider, setAuthProvider] = useState<string | null>(null);
  const [authDeadline, setAuthDeadline] = useState<number | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const [logsOpen, setLogsOpen] = useState(false);
  const [logsRequested, setLogsRequested] = useState(false);
  const [closePrompt, setClosePrompt] = useState(false);
  /** 启动页（赛博鸡蛋孵化舱）：每次进程启动展示一次，进入主界面后不再出现 */
  const [launchDone, setLaunchDone] = useState(false);
  /** 「随时看一眼」封面浮层：顶栏蛋图标打开，Esc/返回 关闭，不丢导航状态 */
  const [coverOpen, setCoverOpen] = useState(false);
  /** WorkBuddy 实时免费模型核对结果（福利提示条用） */
  const [liveFree, setLiveFree] = useState<LiveFreeModels | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [notices, setNotices] = useState<Notice[] | null>(null);
  const [noticeError, setNoticeError] = useState<string | null>(null);
  const [noticesOpen, setNoticesOpen] = useState(false);
  const [markingNotices, setMarkingNotices] = useState(false);
  const toastSeq = useRef(0);
  const refreshSeq = useRef(0);
  const noticesSeq = useRef(0);
  const loaded = useRef(false);
  const storeWarningShown = useRef<string | null>(null);
  const logsSection = useRef<HTMLDivElement>(null);

  const pushToast = useCallback((kind: Toast["kind"], text: string) => {
    const id = ++toastSeq.current;
    setToasts((prev) => [...prev.slice(-3), { id, kind, text }]);
    setTimeout(() => setToasts((prev) => prev.filter((t) => t.id !== id)), 5200);
  }, []);

  const refresh = useCallback(async () => {
    const request = ++refreshSeq.current;
    setLoadError(null);
    // 日志与系统自启状态独立加载，避免它们拖住账号页。
    void api.getLogs(7).then((entries) => {
      if (request === refreshSeq.current) setLogs(entries);
    }).catch((e) => {
      if (request === refreshSeq.current) pushToast("err", `读取签到记录失败：${e}`);
    });
    void api.autostartEnabled().then((enabled) => {
      if (request === refreshSeq.current) setAutostart(enabled);
    }).catch(() => {
      if (request === refreshSeq.current) setAutostart(null);
    });
    try {
      const ov = await api.overview();
      const cfg = await api.getConfig();
      if (request !== refreshSeq.current) return;
      setOverview(ov);
      setConfig(cfg);
      loaded.current = true;
      if (ov.storeWarning && ov.storeWarning !== storeWarningShown.current) {
        storeWarningShown.current = ov.storeWarning;
        pushToast("warn", ov.storeWarning);
      }
      setSelected((prev) =>
        prev === SETTINGS_ID || prev === HOME_ID || ov.providers.some((p) => p.id === prev)
          ? prev
          : HOME_ID
      );
    } catch (e) {
      if (request !== refreshSeq.current) return;
      if (loaded.current) pushToast("err", `刷新账号失败：${e}`);
      else setLoadError(String(e));
    }
  }, [pushToast]);

  const refreshNotices = useCallback(async () => {
    const request = ++noticesSeq.current;
    try {
      const items = await api.listNotices();
      if (request !== noticesSeq.current) return;
      setNotices(items);
      setNoticeError(null);
    } catch (e) {
      if (request === noticesSeq.current) setNoticeError(`读取提醒失败：${e}`);
    }
  }, []);

  const openNotices = useCallback(() => {
    setClosePrompt(false);
    setNoticesOpen(true);
    void refreshNotices();
  }, [refreshNotices]);

  useEffect(() => {
    let active = true;
    const handleRequest = async () => {
      if (!active) return;
      const request = await api.takeWindowRequest();
      if (!active) return;
      // 托盘 / 单实例唤起时直接进主界面，不让启动页挡路；无请求时保持启动页
      if (typeof request === "string") {
        setLaunchDone(true);
        setCoverOpen(false);
      }
      if (request === "settings" || request === "logs") {
        setClosePrompt(false);
        setNoticesOpen(false);
      }
      switch (request) {
        case "close":
          setNoticesOpen(false);
          setClosePrompt(true);
          break;
        case "notices": openNotices(); break;
        case "settings": setSelected(SETTINGS_ID); break;
        case "logs":
          setSelected(HOME_ID);
          setLogsOpen(true);
          setLogsRequested(true);
          break;
      }
    };
    const accounts = onAccountsChanged(() => { if (active) void refresh(); });
    const reminders = onNoticesChanged((notice) => {
      if (!active) return;
      void refreshNotices();
      if (notice && document.visibilityState === "visible" && document.hasFocus()) {
        pushToast(notice.kind, notice.title);
      }
    });
    const windows = onWindowRequested(() => {
      void handleRequest().catch((e) => {
        if (active) pushToast("err", `读取窗口操作失败：${e}`);
      });
    });
    // 先建立订阅，再读取状态，覆盖窗口尚未加载时产生的提醒与托盘操作。
    void reminders.then(() => { if (active) void refreshNotices(); })
      .catch((e) => { if (active) setNoticeError(String(e)); });
    void windows.then(handleRequest)
      .catch((e) => { if (active) pushToast("err", `读取窗口操作失败：${e}`); });
    void accounts.catch((e) => {
      if (active) pushToast("err", `订阅账号更新失败：${e}`);
    }).then(() => { if (active) void refresh(); });
    return () => {
      active = false;
      ++refreshSeq.current;
      ++noticesSeq.current;
      for (const subscription of [accounts, reminders, windows]) {
        void subscription.then((unlisten) => unlisten()).catch(() => {});
      }
    };
  }, [openNotices, refresh, refreshNotices, pushToast]);

  useEffect(() => {
    if (!logsRequested || !logsSection.current) return;
    logsSection.current.scrollIntoView({ block: "start" });
    setLogsRequested(false);
  }, [logsRequested, overview, config]);

  // 前台轮询（积分和签到状态都会变）；间隔在设置里可调，0 = 关闭。
  // config.json 可手改，非零值钳制到 10–3600 秒，避免 1 秒级轮询打爆上游接口。
  const rawUiRefresh = config?.uiRefreshSeconds ?? 45;
  const uiRefreshSeconds =
    rawUiRefresh <= 0 ? 0 : Math.min(3600, Math.max(10, rawUiRefresh));
  useEffect(() => {
    if (uiRefreshSeconds <= 0) return;
    const timer = setInterval(() => void refresh(), uiRefreshSeconds * 1000);
    return () => clearInterval(timer);
  }, [refresh, uiRefreshSeconds]);

  const runAll = async () => {
    setBusy(true);
    try {
      const summary = await api.checkinAll();
      pushToast(summary.kind, summary.message);
    } catch (e) {
      pushToast("err", `签到失败：${e}`);
    } finally {
      setBusy(false);
    }
  };

  const runOne = async (providerId: string, accountId: string, label: string) => {
    setBusy(true);
    try {
      const result = await api.checkinAccount(providerId, accountId);
      if (result.status === "success") pushToast("ok", `${label} +${result.points}`);
      else if (result.status === "already") pushToast("warn", `${label} 今日已签`);
      else pushToast(result.status === "failed" ? "err" : "warn", `${label}：${result.message}`);
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

  /** 启动页按平台领蛋：把该平台所有可签账号依次签完，汇总一条提示 */
  const claimProvider = async (providerId: string) => {
    const targets = (overview?.accounts ?? []).filter(
      (a) => a.account.provider === providerId && a.status?.state === "claimable"
    );
    if (targets.length === 0) return;
    setBusy(true);
    let ok = 0;
    let points = 0;
    let failed = 0;
    try {
      for (const t of targets) {
        try {
          const r = await api.checkinAccount(providerId, t.account.id);
          if (r.status === "success") {
            ok += 1;
            points += r.points;
          } else if (r.status === "already") {
            ok += 1;
          } else {
            failed += 1;
          }
        } catch {
          failed += 1;
        }
      }
      pushToast(
        failed > 0 ? "warn" : "ok",
        failed > 0
          ? `领蛋完成：成功 ${ok} 枚，失败 ${failed} 枚`
          : `领蛋 +${points} 积分（${ok} 枚）`
      );
    } finally {
      setBusy(false);
      await refresh();
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
      const saved = await api.addDiscoveredAccount(providerId);
      // 已在池里的账号这次不会重复入库；一个新账号都没有时，
      // 大概率是「客户端登录的还是老号」—— 直接告诉用户该怎么做
      const fresh = found.filter((f) => !f.alreadyAdded);
      const savedIds = new Set(
        saved.filter((a) => a.provider === providerId).map((a) => a.id)
      );
      const missing = fresh.filter((f) => !savedIds.has(f.account.id));
      if (missing.length > 0) {
        pushToast(
          "err",
          `导入没有写入：${missing.map((f) => f.account.label).join("、")}，请重试`
        );
      } else if (fresh.length > 0) {
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
      setAuthDeadline(
        challenge.expiresInSecs ? Date.now() + challenge.expiresInSecs * 1000 : null,
      );
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
          setAuthDeadline(null);
          pushToast("ok", `已添加账号：${account.label}`);
          await refresh();
        }
      } catch (e) {
        setAuthSession(null);
        setAuthDeadline(null);
        pushToast("err", `授权失败：${e}`);
      }
    }, 2500);
    return () => clearInterval(timer);
  }, [authSession, pushToast, refresh]);

  // 授权倒计时：上游的 device code 只有 5 分钟左右，过期后网页会报
  // 「授权链接无效 / 缺少有效的 user_code」，横幅上把剩余时间显示出来
  useEffect(() => {
    if (!authDeadline) return;
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [authDeadline]);

  // 有待完成的授权时直接进主界面（授权横幅只在主界面里）
  useEffect(() => {
    if (authSession) setLaunchDone(true);
  }, [authSession]);

  // 应用重启后恢复未完成的授权轮询
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const pending = await api.pendingAuth();
        if (!cancelled && pending.length > 0) {
          setAuthSession(pending[0].sessionId);
          setAuthUrl(pending[0].authUrl);
          setAuthDeadline(
            pending[0].expiresInSecs
              ? Date.now() + pending[0].expiresInSecs * 1000
              : null,
          );
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
    setBusy(true);
    try {
      await api.removeAccount(providerId, accountId);
      pushToast("warn", `已移除：${label}`);
      await refresh();
    } catch (e) {
      pushToast("err", `移除失败：${e}`);
    } finally {
      setBusy(false);
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
    const chatSyncEnabled = Boolean(
      overview?.agents[providerId]?.chatSyncSupported &&
      config?.scheduler.agents[providerId]?.chatSyncEnabled
    );
    const providerName = overview?.providers.find((provider) => provider.id === providerId)?.name ?? providerId;
    setBusy(true);
    setSwitchProgress({ providerId, providerName, accountId, accountLabel: label, chatSyncEnabled, stage: "preparing" });
    let unlisten = () => {};
    try {
      unlisten = await onSwitchProgress((progress) => {
        if (progress.providerId === providerId && progress.accountId === accountId) {
          setSwitchProgress((current) => current ? { ...current, stage: progress.stage } : current);
        }
      });
    } catch {
      // 即使进度事件不可用，切换命令仍继续；结果和错误仍由 invoke 返回。
    }
    try {
      const result = await api.switchClientAccount(providerId, accountId);
      const chatNote = result.chatSync ? `；${result.chatSync.detail}` : "";
      setSwitchProgress((current) => current ? { ...current, stage: "completed", detail: result.chatSync?.detail } : current);
      pushToast(
        "ok",
        `${label}：已切换客户端账号并重启，同时设为当前账号${result.backup ? "（原登录态已备份）" : ""}${chatNote}`
      );
      await refresh();
    } catch (e) {
      setSwitchProgress((current) => current ? { ...current, stage: "failed", error: String(e) } : current);
    } finally {
      unlisten();
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

  /** 仅保存本次改动的调度字段，校验通过后更新显示。 */
  const patchSchedule = useCallback(
    (providerId: string, patch: { enabled?: boolean; startMinute?: number; endMinute?: number }) => {
      void api
        .setAgentSchedule(providerId, patch)
        .then((cfg) => {
          setConfig(cfg);
          setOverview((prev) => {
            const agent = prev?.agents[providerId];
            if (!prev || !agent) return prev;
            return { ...prev, agents: { ...prev.agents, [providerId]: { ...agent, ...patch } } };
          });
        })
        .catch((e) => pushToast("err", `保存失败：${e}`));
    },
    [pushToast]
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

  /** 界面自动刷新间隔（秒）：乐观更新 + 落盘，轮询定时器随 config 重建 */
  const patchUiRefreshInterval = useCallback(
    (seconds: number) => {
      setConfig((prev) => (prev ? { ...prev, uiRefreshSeconds: seconds } : prev));
      void api
        .setUiRefreshInterval(seconds)
        .then((cfg) => setConfig(cfg))
        .catch((e) => pushToast("err", `保存失败：${e}`));
    },
    [pushToast]
  );

  /** 实时核对 WorkBuddy 当前免费模型（模型目录与客户端对话选择器同源） */
  const refreshFreeModels = useCallback(async () => {
    setLiveFree((prev) => ({
      models: prev?.models ?? [],
      fetchedAt: prev?.fetchedAt ?? 0,
      loading: true,
      error: null,
    }));
    try {
      const models = await api.fetchWorkbuddyFreeModels();
      setLiveFree({ models, fetchedAt: Date.now(), loading: false, error: null });
    } catch (e) {
      setLiveFree((prev) => ({
        models: prev?.models ?? [],
        fetchedAt: prev?.fetchedAt ?? 0,
        loading: false,
        error: String(e),
      }));
    }
  }, []);

  // 进入 WorkBuddy 面板时自动核对一次最新免费模型
  useEffect(() => {
    if (selected === "workbuddy" && !liveFree) void refreshFreeModels();
  }, [selected, liveFree, refreshFreeModels]);

  /** 平台显示开关：乐观更新 + 落盘；隐藏侧栏时若正停留在该平台页则回首页 */
  const patchVisibility = useCallback(
    (providerId: string, patch: Partial<AgentVisibility>) => {
      setConfig((prev) => {
        if (!prev) return prev;
        const cur = prev.agentVisibility[providerId] ?? { sidebar: true, home: true };
        return {
          ...prev,
          agentVisibility: {
            ...prev.agentVisibility,
            [providerId]: { ...cur, ...patch },
          },
        };
      });
      if (patch.sidebar === false) {
        setSelected((prev) => (prev === providerId ? HOME_ID : prev));
      }
      void api
        .setAgentVisibility(providerId, patch)
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

  const toggleCatchUp = async (enabled: boolean) => {
    try {
      const cfg = await api.setCatchUpMissedWindow(enabled);
      setConfig(cfg);
      pushToast(
        "ok",
        enabled ? "错过时间窗自动补签已开启" : "错过时间窗自动补签已关闭"
      );
    } catch (e) {
      pushToast("err", `${e}`);
    }
  };

  const syncNow = async () => {
    setSyncing(true);
    try {
      const summary = await api.syncNow();
      pushToast(summary.kind, summary.message);
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

  /** 开启 / 关闭开机自启（真实状态以操作系统为准） */
  const changeAutostart = useCallback(
    async (enabled: boolean) => {
      try {
        const actual = await api.setAutostart(enabled);
        setAutostart(actual);
        pushToast(
          "ok",
          actual ? "已开启开机启动（静默驻留托盘）" : "已关闭开机启动"
        );
      } catch (e) {
        pushToast("err", `设置开机启动失败：${e}`);
      }
    },
    [pushToast]
  );

  /** 自动提醒总开关 */
  const changeNotifications = useCallback(
    async (enabled: boolean) => {
      try {
        const cfg = await api.setNotificationsEnabled(enabled);
        setConfig(cfg);
        pushToast("ok", enabled ? "自动提醒已开启" : "自动提醒已关闭");
      } catch (e) {
        pushToast("err", `保存失败：${e}`);
      }
    },
    [pushToast]
  );

  const testNotification = useCallback(async () => {
    try {
      await api.sendTestNotification();
      openNotices();
    } catch (e) {
      pushToast("err", `发送测试提醒失败：${e}`);
    }
  }, [openNotices, pushToast]);

  const markNoticesRead = async () => {
    const ids = notices?.filter((notice) => !notice.read).map((notice) => notice.id) ?? [];
    if (!ids.length) return;
    setMarkingNotices(true);
    try {
      await api.markNoticesRead(ids);
      await refreshNotices();
    } catch (e) {
      setNoticeError(`标为已读失败：${e}`);
    } finally {
      setMarkingNotices(false);
    }
  };

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

  const unread = notices?.filter((notice) => !notice.read).length ?? 0;
  const noticesButton = (
    <button className="btn notices-button" onClick={openNotices} aria-haspopup="dialog"
      aria-label={unread ? `应用内提醒，${unread} 条未读` : "应用内提醒"}
    >
      提醒
      {unread > 0 && <span className="notices-badge" aria-hidden="true">{unread}</span>}
    </button>
  );
  const overlays = (
    <>
      <div className="toasts" role="status" aria-live="polite">
        {toasts.map((t) => (
          <div key={t.id} className={`toast ${t.kind}`}>{t.text}</div>
        ))}
      </div>
      {closePrompt && (
        <CloseConfirmDialog
          onCancel={() => setClosePrompt(false)}
          onResolve={(action, remember) => void resolveClose(action, remember)}
        />
      )}
      {noticesOpen && (
        <NotificationsDialog
          notices={notices}
          error={noticeError}
          marking={markingNotices}
          onMarkRead={() => void markNoticesRead()}
          onRetry={() => void refreshNotices()}
          onClose={() => setNoticesOpen(false)}
        />
      )}
      {switchProgress && (
        <SwitchProgressDialog
          providerName={switchProgress.providerName}
          accountLabel={switchProgress.accountLabel}
          stage={switchProgress.stage}
          chatSyncEnabled={switchProgress.chatSyncEnabled}
          detail={switchProgress.detail}
          error={switchProgress.error}
          onClose={() => setSwitchProgress(null)}
        />
      )}
    </>
  );

  // 启动页：赛博鸡蛋孵化舱。进入主界面后本次进程不再展示（托盘唤醒不重弹）。
  if (!launchDone) {
    return (
      <div className="shell launch-shell">
        <LaunchScreen
          overview={overview}
          loadError={loadError}
          busy={busy}
          visibility={config?.agentVisibility ?? {}}
          onClaimAll={() => void runAll()}
          onClaimProvider={(id) => void claimProvider(id)}
          onOpenProvider={(id) => {
            setLaunchDone(true);
            setSelected(id);
          }}
          onEnter={() => setLaunchDone(true)}
          onRetry={() => void refresh()}
        />
        {overlays}
      </div>
    );
  }

  // 首屏骨架：不等 overview（它要等各平台网络请求，3 秒级）。
  // 立即渲染界面结构 + 骨架占位，数据到了自动填充（定时轮询 / 事件驱动）。
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
            {noticesButton}
          </header>
          {loadError ? (
            <section className="panel load-error" role="alert">
              <h2>账号与积分加载失败</h2>
              <p>{loadError}</p>
              <button className="btn primary" onClick={() => void refresh()}>重试</button>
            </section>
          ) : (
            <>
              <section className="panel dash" aria-hidden="true">
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
              <div className="loading-hint" role="status">
                <div className="loading-mark" />
                <p>正在加载账号与积分…</p>
              </div>
            </>
          )}
        </div>
        {overlays}
      </div>
    );
  }

  const groups = buildGroups(overview);
  // 侧栏只列「侧栏显示」的平台；首页卡片在 DashboardPanel 内按 home 开关过滤。
  // current 仍用全量 groups：首页卡片点进被侧栏隐藏的平台页也允许。
  const sidebarGroups = groups.filter(
    (g) => config.agentVisibility[g.provider.id]?.sidebar !== false
  );
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
  // 授权码剩余秒数（上游 device code 只有 5 分钟左右）
  const authLeft = authDeadline
    ? Math.max(0, Math.round((authDeadline - now) / 1000))
    : null;
  const authExpired = authLeft !== null && authLeft <= 0;

  return (
    <div className="shell">
      <Sidebar
        groups={sidebarGroups}
        selected={selected}
        onSelect={setSelected}
        onOpenHome={() => setSelected(HOME_ID)}
        onOpenSettings={() => setSelected(SETTINGS_ID)}
        providers={overview.providers}
        visibility={config.agentVisibility}
        onVisibilityChange={patchVisibility}
        onReorder={(order) => {
          // order 只含当前可见平台：隐藏的平台保持原槽位，合并成完整顺序再落盘
          const visible = new Set(order);
          const queue = [...order];
          const merged = overview.providers.map((p) =>
            visible.has(p.id) ? (queue.shift() as string) : p.id
          );
          // 乐观更新本地顺序，落盘失败再回滚刷新
          setOverview((prev) =>
            prev
              ? {
                  ...prev,
                  providers: merged
                    .map((id) => prev.providers.find((p) => p.id === id))
                    .filter((p): p is NonNullable<typeof p> => !!p),
                }
              : prev
          );
          void api
            .setAgentOrder(merged)
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
                {authExpired ? (
                  <>授权码已过期，网页会提示「缺少有效的 user_code」。</>
                ) : (
                  <>已在浏览器打开授权页。</>
                )}
                {authLeft !== null && !authExpired && <>（{authLeft} 秒内有效）</>}
                <button className="link" onClick={() => api.openExternal(authUrl)}>
                  重新打开
                </button>
                用要登录的账号点「确认授权」。
              </span>
            </div>
            {authExpired ? (
              <button
                className="btn small"
                onClick={() => {
                  const provider = authProvider ?? "qoder";
                  setAuthSession(null);
                  setAuthProvider(null);
                  setAuthDeadline(null);
                  void startDeviceAuth(provider);
                }}
              >
                重新发起
              </button>
            ) : (
              <button
                className="btn small"
                onClick={() => {
                  setAuthSession(null);
                  setAuthProvider(null);
                  setAuthDeadline(null);
                }}
              >
                取消
              </button>
            )}
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
            {noticesButton}
            <button
              className="cover-button"
              onClick={() => setCoverOpen(true)}
              aria-label="看一眼赛博鸡蛋"
              title="看一眼赛博鸡蛋（启动页封面）"
            >
              <svg viewBox="0 0 120 150" fill="none" stroke="currentColor" strokeWidth="10" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <path d="M60 8 C88 8 108 44 108 82 C108 116 86 142 60 142 C34 142 12 116 12 82 C12 44 32 8 60 8 Z" />
              </svg>
            </button>
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
            visibility={config.agentVisibility}
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
            onCatchUpMissedWindow={toggleCatchUp}
            onRefreshInterval={patchRefreshInterval}
            onUiRefreshInterval={patchUiRefreshInterval}
            onScheduleChange={(id, patch) => patchSchedule(id, patch)}
            onSyncNow={syncNow}
            onOpenDataDir={openDataDir}
            onCloseAction={(action) => void changeCloseAction(action)}
            autostartEnabled={autostart}
            onAutostartChange={(enabled) => void changeAutostart(enabled)}
            onNotificationsEnabled={(enabled) => void changeNotifications(enabled)}
            onTestNotification={() => void testNotification()}
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
            liveFree={liveFree}
            onRefreshFree={() => void refreshFreeModels()}
          />
        ) : (
          <div className="empty-state">
            <p>还没有任何 agent 分组。先在左侧选择一个平台添加账号。</p>
          </div>
        )}

        <div ref={logsSection}>
          <LogPanel
            logs={logs}
            providerId={current?.provider.id ?? null}
            providerName={current?.provider.name ?? "全部"}
            providerNames={providerNames}
            accountNames={accountNames}
            visible={logsOpen}
            onToggle={() => setLogsOpen((v) => !v)}
          />
        </div>
      </div>
      {coverOpen && (
        <div className="cover-overlay" role="dialog" aria-label="赛博鸡蛋封面">
          <LaunchScreen
            overview={overview}
            loadError={loadError}
            busy={busy}
            visibility={config.agentVisibility}
            onClaimAll={() => void runAll()}
            onClaimProvider={(id) => void claimProvider(id)}
            onOpenProvider={(id) => {
              setCoverOpen(false);
              setSelected(id);
            }}
            onEnter={() => setCoverOpen(false)}
            onBack={() => setCoverOpen(false)}
            onRetry={() => void refresh()}
          />
        </div>
      )}
      {overlays}
    </div>
  );
}
