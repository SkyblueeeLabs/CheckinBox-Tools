import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  Account,
  AccountResult,
  AppConfig,
  AuthChallenge,
  DiscoveredAccount,
  LogEntry,
  Overview,
  Quota,
} from "./types";

export const api = {
  overview: () => invoke<Overview>("overview"),
  checkinAll: () => invoke<AccountResult[]>("checkin_all"),
  checkinAccount: (providerId: string, accountId: string) =>
    invoke<AccountResult>("checkin_account", { providerId, accountId }),
  refreshQuota: (providerId: string, accountId: string) =>
    invoke<Quota>("refresh_quota", { providerId, accountId }),
  refreshAllQuotas: () =>
    invoke<Array<[string, string, Quota]>>("refresh_all_quotas"),
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
  setAgentSchedule: (
    providerId: string,
    enabled: boolean,
    startMinute: number,
    endMinute: number
  ) =>
    invoke<AppConfig>("set_agent_schedule", {
      providerId,
      enabled,
      startMinute,
      endMinute,
    }),
  setSchedulerEnabled: (enabled: boolean) =>
    invoke<AppConfig>("set_scheduler_enabled", { enabled }),
  setActiveAccount: (providerId: string, accountId: string) =>
    invoke<AppConfig>("set_active_account", { providerId, accountId }),
  launchClient: (providerId: string) =>
    invoke<string>("launch_client", { providerId }),
  getLogs: (days = 7) => invoke<LogEntry[]>("get_logs", { days }),
  openExternal: (url: string) => invoke<void>("open_external", { url }),
};

export function onCheckinProgress(
  handler: (result: AccountResult) => void
): Promise<UnlistenFn> {
  return listen<AccountResult>("checkin-progress", (event) => handler(event.payload));
}

/** 分钟数 -> HH:MM */
export function formatMinute(minute: number): string {
  const m = Math.max(0, Math.min(24 * 60 - 1, Math.round(minute)));
  return `${String(Math.floor(m / 60)).padStart(2, "0")}:${String(m % 60).padStart(2, "0")}`;
}

/** HH:MM -> 分钟数 */
export function parseMinute(text: string): number | null {
  const match = /^(\d{1,2}):(\d{2})$/.exec(text.trim());
  if (!match) return null;
  const h = Number(match[1]);
  const m = Number(match[2]);
  if (h > 23 || m > 59) return null;
  return h * 60 + m;
}

/** 积分数取整显示；顺带把 -0 归一成 0 */
export function fmtPoints(n: number): string {
  if (!Number.isFinite(n)) return "0";
  const rounded = Math.round(n);
  const norm = rounded === 0 ? 0 : rounded;
  return norm.toLocaleString("zh-CN");
}

/** 毫秒时间戳 -> YYYY-MM-DD */
export function fmtDate(ms: number): string {
  if (!ms) return "";
  const d = new Date(ms);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(
    d.getDate()
  ).padStart(2, "0")}`;
}
