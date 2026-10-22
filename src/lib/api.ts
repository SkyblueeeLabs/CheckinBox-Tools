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
} from "./types";

export const api = {
  overview: () => invoke<Overview>("overview"),
  checkinAll: () => invoke<AccountResult[]>("checkin_all"),
  checkinAccount: (providerId: string, accountId: string) =>
    invoke<AccountResult>("checkin_account", { providerId, accountId }),
  discoverAccounts: () => invoke<DiscoveredAccount[]>("discover_accounts"),
  addDiscoveredAccount: (providerId: string) =>
    invoke<Account[]>("add_discovered_account", { providerId }),
  beginQoderAuth: () => invoke<AuthChallenge>("begin_qoder_auth"),
  pollQoderAuth: (sessionId: string) =>
    invoke<Account | null>("poll_qoder_auth", { sessionId }),
  removeAccount: (providerId: string, accountId: string) =>
    invoke<Account[]>("remove_account", { providerId, accountId }),
  getConfig: () => invoke<AppConfig>("get_config"),
  setConfig: (config: AppConfig) => invoke<AppConfig>("set_config", { config }),
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
