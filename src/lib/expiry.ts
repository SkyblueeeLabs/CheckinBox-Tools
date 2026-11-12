import type { AccountView, CreditItem, Quota } from "./types";

/** 到期提醒窗口（天）。后端 Rust 侧用同一口径发通知。 */
export const EXPIRY_WINDOW_DAYS = 7;
/** 2100-01-01：超过这个时间视为「长期有效」（Qoder 会返回 9999 年）。 */
export const LONG_TERM_MS = Date.UTC(2100, 0, 1);
const DAY_MS = 86_400_000;

export interface ExpiringCredits {
  /** 窗口内到期的剩余积分合计 */
  remaining: number;
  /** 最早一批的到期时刻 */
  earliestMs: number;
  /** 距最早到期还有几天（向上取整，最小 1） */
  daysLeft: number;
}

/** 7 天内到期的剩余积分；没有则返回 null。 */
export function expiringCredits(
  quota: Quota | null | undefined,
  now = Date.now()
): ExpiringCredits | null {
  if (!quota || quota.items.length === 0) return null;
  const limit = now + EXPIRY_WINDOW_DAYS * DAY_MS;
  let remaining = 0;
  let earliest = Number.POSITIVE_INFINITY;
  for (const item of quota.items) {
    if (!(item.remaining > 0)) continue;
    if (!item.expireAtMs || item.expireAtMs <= now || item.expireAtMs > limit) continue;
    remaining += item.remaining;
    if (item.expireAtMs < earliest) earliest = item.expireAtMs;
  }
  if (!Number.isFinite(earliest)) return null;
  return {
    remaining,
    earliestMs: earliest,
    daysLeft: Math.max(1, Math.ceil((earliest - now) / DAY_MS)),
  };
}

export interface ExpiringAccount {
  view: AccountView;
  credits: ExpiringCredits;
}

/** 单个批次的到期展示文案：未提供 / 长期有效 / YYYY-MM-DD。 */
export function expiryLabel(ms: number): string {
  if (!ms || ms <= 0) return "未提供";
  if (ms >= LONG_TERM_MS) return "长期有效";
  const d = new Date(ms);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(
    d.getDate()
  ).padStart(2, "0")}`;
}

export type ExpirySummary =
  | { kind: "none" }
  | { kind: "long" }
  | { kind: "dated"; earliestMs: number; remaining: number };

/** 汇总有剩余的批次：最近到期 / 长期有效 / 平台未提供。 */
export function summarizeExpiry(items: CreditItem[]): ExpirySummary {
  const live = items.filter((i) => i.remaining > 0 && i.expireAtMs > 0);
  const dated = live.filter((i) => i.expireAtMs < LONG_TERM_MS);
  if (dated.length > 0) {
    const earliest = dated.reduce((a, b) => (b.expireAtMs < a.expireAtMs ? b : a));
    return {
      kind: "dated",
      earliestMs: earliest.expireAtMs,
      remaining: earliest.remaining,
    };
  }
  if (live.length > 0) return { kind: "long" };
  return { kind: "none" };
}

/** 按紧迫度排序（天数少的在前，同天数剩余多的在前）。 */
export function expiringAccounts(
  accounts: AccountView[],
  now = Date.now()
): ExpiringAccount[] {
  const out: ExpiringAccount[] = [];
  for (const view of accounts) {
    const credits = expiringCredits(view.quota, now);
    if (credits) out.push({ view, credits });
  }
  return out.sort(
    (a, b) =>
      a.credits.daysLeft - b.credits.daysLeft ||
      b.credits.remaining - a.credits.remaining
  );
}
