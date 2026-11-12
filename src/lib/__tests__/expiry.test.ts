import { describe, expect, it } from "vitest";
import type { AccountView, CreditItem, Quota } from "../types";
import {
  expiryLabel,
  expiringAccounts,
  expiringCredits,
  LONG_TERM_MS,
  summarizeExpiry,
} from "../expiry";

const DAY = 86_400_000;
const NOW = 1_700_000_000_000;

function item(remaining: number, expireAtMs: number): CreditItem {
  return {
    creditType: "free",
    granted: remaining,
    consumed: 0,
    remaining,
    grantedAtMs: 0,
    expireAtMs,
  };
}

function quota(items: CreditItem[]): Quota {
  return {
    totalGranted: 0,
    totalConsumed: 0,
    totalRemaining: 0,
    items,
    message: "",
  };
}

function view(id: string, q: Quota): AccountView {
  return {
    account: { id, provider: "qoder", label: id, createdAt: 0, origin: "" },
    providerName: "Qoder CN",
    accent: "#000",
    status: null,
    quota: q,
    isActive: false,
    isClientLogin: false,
    projectCount: 0,
    sessionCount: 0,
    isLow: false,
    alertThreshold: 200,
    trend: [],
    supportsClaim: true,
  };
}

describe("expiringCredits", () => {
  it("只统计 7 天内到期且未用完的批次，取最早到期日", () => {
    const got = expiringCredits(
      quota([
        item(100, NOW + 2 * DAY),
        item(50, NOW + 6 * DAY),
        item(0, NOW + 1 * DAY),
        item(30, NOW + 8 * DAY),
        item(20, NOW - DAY),
        item(40, 0),
      ]),
      NOW
    );
    expect(got).toEqual({ remaining: 150, earliestMs: NOW + 2 * DAY, daysLeft: 2 });
  });

  it("没有窗口内批次时返回 null", () => {
    expect(expiringCredits(null, NOW)).toBeNull();
    expect(expiringCredits(quota([]), NOW)).toBeNull();
    expect(expiringCredits(quota([item(999, NOW + 30 * DAY)]), NOW)).toBeNull();
  });

  it("不足一天按 1 天展示", () => {
    const got = expiringCredits(quota([item(10, NOW + 1000)]), NOW);
    expect(got?.daysLeft).toBe(1);
  });
});

describe("expiringAccounts", () => {
  it("按紧迫度排序：天数少的在前，同天数剩余多的在前", () => {
    const sorted = expiringAccounts(
      [
        view("a", quota([item(100, NOW + 5 * DAY)])),
        view("b", quota([item(300, NOW + 1 * DAY)])),
        view("c", quota([item(500, NOW + 5 * DAY)])),
        view("d", quota([])),
      ],
      NOW
    );
    expect(sorted.map((x) => x.view.account.id)).toEqual(["b", "c", "a"]);
  });
});

describe("expiryLabel / summarizeExpiry", () => {
  it("0 显示未提供，9999 年显示长期有效，其余显示日期", () => {
    expect(expiryLabel(0)).toBe("未提供");
    expect(expiryLabel(LONG_TERM_MS + DAY)).toBe("长期有效");
    const ms = new Date(2026, 10, 20).getTime();
    expect(expiryLabel(ms)).toBe("2026-11-20");
  });

  it("优先汇总最近的到期批次，否则长期/未提供", () => {
    expect(summarizeExpiry([item(100, NOW + 3 * DAY), item(50, NOW + 1 * DAY)])).toEqual({
      kind: "dated",
      earliestMs: NOW + DAY,
      remaining: 50,
    });
    expect(summarizeExpiry([item(100, LONG_TERM_MS + DAY)])).toEqual({ kind: "long" });
    expect(summarizeExpiry([item(100, 0)])).toEqual({ kind: "none" });
    expect(summarizeExpiry([item(0, NOW + DAY)])).toEqual({ kind: "none" });
  });
});
