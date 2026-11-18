import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  Account,
  AccountResult,
  AppConfig,
  AuthChallenge,
  ChatBackupEntry,
  ClientAccountView,
  CloseAction,
  CreditPackagePage,
  CreditUsagePage,
  DiscoveredAccount,
  FreeModel,
  LogEntry,
  Notice,
  Overview,
  Quota,
  SwitchOutcome,
  SwitchProgress,
  TaskSummary,
  WindowRequest,
} from "./types";

export const api = {
  overview: () => invoke<Overview>("overview"),
  checkinAll: () => invoke<TaskSummary>("checkin_all"),
  checkinAccount: (providerId: string, accountId: string) =>
    invoke<AccountResult>("checkin_account", { providerId, accountId }),
  refreshQuota: (providerId: string, accountId: string) =>
    invoke<Quota>("refresh_quota", { providerId, accountId }),
  refreshAllQuotas: () =>
    invoke<Array<[string, string, Quota]>>("refresh_all_quotas"),
  workbuddyCreditPackages: (
    accountId: string,
    kind: string,
    expired: boolean,
    page: number
  ) =>
    invoke<CreditPackagePage>("workbuddy_credit_packages", {
      accountId,
      kind,
      expired,
      page,
    }),
  workbuddyCreditUsage: (accountId: string) =>
    invoke<CreditUsagePage>("workbuddy_credit_usage", { accountId }),
  discoverAccounts: () => invoke<DiscoveredAccount[]>("discover_accounts"),
  addDiscoveredAccount: (providerId: string) =>
    invoke<Account[]>("add_discovered_account", { providerId }),
  beginDeviceAuth: (providerId: string) =>
    invoke<AuthChallenge>("begin_device_auth", { providerId }),
  pendingAuth: () => invoke<AuthChallenge[]>("pending_auth"),
  pollDeviceAuth: (providerId: string, sessionId: string) =>
    invoke<Account | null>("poll_device_auth", { providerId, sessionId }),
  removeAccount: (providerId: string, accountId: string) =>
    invoke<Account[]>("remove_account", { providerId, accountId }),
  getConfig: () => invoke<AppConfig>("get_config"),
  setConfig: (config: AppConfig) => invoke<AppConfig>("set_config", { config }),
  setCloseAction: (action: CloseAction) =>
    invoke<AppConfig>("set_close_action", { action }),
  resolveCloseAction: (action: CloseAction, remember: boolean) =>
    invoke<void>("resolve_close_action", { action, remember }),
  autostartEnabled: () => invoke<boolean>("autostart_enabled"),
  setAutostart: (enabled: boolean) => invoke<boolean>("set_autostart", { enabled }),
  setNotificationsEnabled: (enabled: boolean) =>
    invoke<AppConfig>("set_notifications_enabled", { enabled }),
  sendTestNotification: () => invoke<void>("send_test_notification"),
  listNotices: () => invoke<Notice[]>("list_notices"),
  markNoticesRead: (ids: string[]) => invoke<void>("mark_notices_read", { ids }),
  takeWindowRequest: () => invoke<WindowRequest | null>("take_window_request"),
  setAgentOrder: (order: string[]) =>
    invoke<AppConfig>("set_agent_order", { order }),
  setAgentVisibility: (
    providerId: string,
    patch: { sidebar?: boolean; home?: boolean; launch?: boolean }
  ) =>
    invoke<AppConfig>("set_agent_visibility", {
      providerId,
      sidebar: patch.sidebar ?? null,
      home: patch.home ?? null,
      launch: patch.launch ?? null,
    }),
  fetchWorkbuddyFreeModels: (accountId?: string) =>
    invoke<FreeModel[]>("fetch_workbuddy_free_models", {
      accountId: accountId ?? null,
    }),
  setAgentSchedule: (
    providerId: string,
    patch: { enabled?: boolean; startMinute?: number; endMinute?: number },
  ) =>
    invoke<AppConfig>("set_agent_schedule", { providerId, patch }),
  setSchedulerEnabled: (enabled: boolean) =>
    invoke<AppConfig>("set_scheduler_enabled", { enabled }),
  setSyncEnabled: (enabled: boolean) =>
    invoke<AppConfig>("set_sync_enabled", { enabled }),
  setCatchUpMissedWindow: (enabled: boolean) =>
    invoke<AppConfig>("set_catch_up_missed_window", { enabled }),
  setRefreshInterval: (providerId: string, minutes: number) =>
    invoke<AppConfig>("set_refresh_interval", { providerId, minutes }),
  setUiRefreshInterval: (seconds: number) =>
    invoke<AppConfig>("set_ui_refresh_interval", { seconds }),
  syncNow: () => invoke<TaskSummary>("sync_now"),
  openDataDir: () => invoke<void>("open_data_dir"),
  setActiveAccount: (providerId: string, accountId: string) =>
    invoke<AppConfig>("set_active_account", { providerId, accountId }),
  setAlertThreshold: (providerId: string, value: number) =>
    invoke<AppConfig>("set_alert_threshold", { providerId, value }),
  setChatSync: (providerId: string, enabled: boolean) =>
    invoke<AppConfig>("set_chat_sync", { providerId, enabled }),
  listChatBackups: () => invoke<ChatBackupEntry[]>("list_chat_backups"),
  deleteChatBackup: (provider: string, opId: string) =>
    invoke<ChatBackupEntry[]>("delete_chat_backup", { provider, opId }),
  deleteOldChatBackups: (days: number) =>
    invoke<ChatBackupEntry[]>("delete_old_chat_backups", { days }),
  launchClient: (providerId: string) =>
    invoke<string>("launch_client", { providerId }),
  detectClientAccount: (providerId: string) =>
    invoke<ClientAccountView | null>("detect_client_account", { providerId }),
  clientRunning: (providerId: string) =>
    invoke<boolean>("client_running", { providerId }),
  switchClientAccount: (providerId: string, accountId: string) =>
    invoke<SwitchOutcome>("switch_client_account", { providerId, accountId }),
  getLogs: (days = 7) => invoke<LogEntry[]>("get_logs", { days }),
  openExternal: (url: string) => invoke<void>("open_external", { url }),
};

export function onNoticesChanged(handler: (notice: Notice | null) => void): Promise<UnlistenFn> {
  return listen<Notice | null>("notices-changed", ({ payload }) => handler(payload));
}

export function onAccountsChanged(handler: () => void): Promise<UnlistenFn> {
  return listen("accounts-changed", () => handler());
}

export function onSwitchProgress(handler: (progress: SwitchProgress) => void): Promise<UnlistenFn> {
  return listen<SwitchProgress>("switch-progress", ({ payload }) => handler(payload));
}

export function onWindowRequested(handler: () => void): Promise<UnlistenFn> {
  return listen("window-requested", () => handler());
}

/** 分钟数 -> HH:MM。允许 24:00（后端 is_valid 允许 end_minute <= 24*60）。 */
export function formatMinute(minute: number): string {
  const m = Math.max(0, Math.min(24 * 60, Math.round(minute)));
  return `${String(Math.floor(m / 60)).padStart(2, "0")}:${String(m % 60).padStart(2, "0")}`;
}

/** HH:MM -> 分钟数。分钟允许 1-2 位（"10:3" 也认），并允许 24:00。 */
export function parseMinute(text: string): number | null {
  const match = /^(\d{1,2}):(\d{1,2})$/.exec(text.trim());
  if (!match) return null;
  const h = Number(match[1]);
  const m = Number(match[2]);
  if (m > 59) return null;
  if (h === 24 && m === 0) return 24 * 60;
  if (h > 23) return null;
  return h * 60 + m;
}

/** 积分数取整显示；顺带把 -0 归一成 0 */
export function fmtPoints(n: number): string {
  if (!Number.isFinite(n)) return "0";
  const rounded = Math.round(n);
  const norm = rounded === 0 ? 0 : rounded;
  return norm.toLocaleString("zh-CN");
}

/** WorkBuddy 积分按次计费有小数（0.86/次）：保留最多两位，千分位，-0 归一 */
export function fmtCredits(n: number): string {
  if (!Number.isFinite(n)) return "0";
  const rounded = Math.round(n * 100) / 100;
  const norm = rounded === 0 ? 0 : rounded;
  return norm.toLocaleString("zh-CN", { maximumFractionDigits: 2 });
}

/** 额度按 Token 展示的平台（每日额度是 3 亿级 Token，不是积分） */
const TOKEN_PROVIDERS = new Set(["zcode"]);

/** 该平台的额度单位：Zcode 是 Token，其它平台是积分 */
export function quotaUnit(providerId?: string | null): "Token" | "积分" {
  return providerId && TOKEN_PROVIDERS.has(providerId) ? "Token" : "积分";
}

/** 面板里「剩余 / 累计发放」的标签文案（Zcode 说 Token，其它平台说积分） */
export function quotaLabels(providerId?: string | null): {
  unit: string;
  remaining: string;
  granted: string;
} {
  return quotaUnit(providerId) === "Token"
    ? { unit: "Token", remaining: "剩余 Token", granted: "累计发放 Token" }
    : { unit: "积分", remaining: "剩余总积分", granted: "累计发放总积分" };
}

/** 大数缩写：300000000 → 3亿、298890433 → 2.99亿、12345 → 1.2万（末尾 0 舍去） */
export function fmtCompact(n: number): string {
  if (!Number.isFinite(n)) return "0";
  const abs = Math.abs(n);
  const trim = (v: number, digits: number) => {
    let s = v.toFixed(digits);
    if (s.includes(".")) s = s.replace(/\.?0+$/, "");
    return s;
  };
  if (abs >= 1e8) return `${trim(n / 1e8, 2)}亿`;
  if (abs >= 1e4) return `${trim(n / 1e4, 1)}万`;
  return fmtPoints(n);
}

/** 额度数值展示：Token 平台用缩写，WorkBuddy 保留两位小数，其它积分平台千分位取整 */
export function fmtQuotaAmount(n: number, providerId?: string | null): string {
  if (quotaUnit(providerId) === "Token") return fmtCompact(n);
  if (providerId === "workbuddy") return fmtCredits(n);
  return fmtPoints(n);
}

/** 毫秒时间戳 -> YYYY-MM-DD */
export function fmtDate(ms: number): string {
  if (!ms) return "";
  const d = new Date(ms);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(
    d.getDate()
  ).padStart(2, "0")}`;
}

/** 毫秒时间戳 -> YYYY-MM-DD HH:mm */
export function fmtDateTime(ms: number): string {
  if (!ms) return "时间未知";
  const d = new Date(ms);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(
    d.getHours()
  )}:${p(d.getMinutes())}`;
}

/** 字节数 -> 人类可读（KB/MB，1024 进制） */
export function fmtBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  if (bytes < 1024) return `${Math.round(bytes)} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`;
}
