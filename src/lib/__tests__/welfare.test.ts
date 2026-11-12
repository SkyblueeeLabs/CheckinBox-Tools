import { describe, expect, it } from "vitest";
import { welfareOf } from "../welfare";
import type { AccountView, AgentGroup, CheckinStatus, ProviderView, Quota } from "../types";

function provider(id: string, supportsClaim: boolean): ProviderView {
  return {
    id,
    name: id,
    accent: "#000000",
    docsUrl: "",
    supportsDiscovery: false,
    supportsOAuth: false,
    configured: true,
    hasClient: true,
    supportsClaim,
  };
}

function quota(granted: number): Quota {
  return {
    totalGranted: granted,
    totalConsumed: 0,
    totalRemaining: granted,
    items: [],
    message: "",
  };
}

function account(points: number | null, granted = 0): AccountView {
  const status: CheckinStatus | null =
    points === null
      ? null
      : {
          state: points > 0 ? "claimed" : "claimable",
          points,
          streakDays: 0,
          message: "",
          bonusPoints: 0,
        };
  return {
    account: { id: "a1", provider: "x", label: "账号", createdAt: 0, origin: "" },
    providerName: "X",
    accent: "#000000",
    status,
    quota: granted > 0 ? quota(granted) : null,
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

function group(
  providerId: string,
  supportsClaim: boolean,
  accounts: AccountView[]
): AgentGroup {
  return {
    provider: provider(providerId, supportsClaim),
    accounts,
    totalRemaining: 0,
    totalGranted: 0,
    claimableCount: 0,
    claimedToday: 0,
    schedule: {
      enabled: true,
      startMinute: 360,
      endMinute: 480,
      accountCount: accounts.length,
      todayClaimed: 0,
      todayClaimable: 0,
      refreshIntervalMinutes: 10,
      switchable: false,
      chatSyncEnabled: false,
      chatSyncSupported: false,
    },
  };
}

describe("welfareOf（首页福利画像：单账号每天多少 / 自动还是手动 / 节奏）", () => {
  it("单账号金额：各账号不同时给区间，多账号合计放悬浮说明", () => {
    const w = welfareOf(group("minimax", true, [account(1000), account(400)]));
    expect(w.label).toBe("单账号 +400~1,000 积分/日");
    expect(w.mode).toBe("auto");
    expect(w.note).toBe("06:00-08:00");
    expect(w.title).toContain("合计 1,400 积分");
    expect(w.title).toContain("06:00-08:00");
  });

  it("单账号金额一致时只报一个数", () => {
    const w = welfareOf(group("qoder", true, [account(100), account(100)]));
    expect(w.label).toBe("单账号 +100 Credits/日");
    expect(w.mode).toBe("auto");
    expect(w.note).toBe("10:00 后开放 · 06:00-08:00");
    expect(w.title).toContain("合计 200 Credits");
  });

  it("单账号时合计不重复显示", () => {
    const w = welfareOf(group("qoder", true, [account(100)]));
    expect(w.title).toContain("单账号每日 100 Credits");
    expect(w.title).not.toContain("合计");
  });

  it("已领取也要报数（qoder 已领取时 status.points 带当天积分）", () => {
    const w = welfareOf(group("qoder", true, [account(100), account(100)]));
    expect(w.label).toBe("单账号 +100 Credits/日");
  });

  it("Zcode：单账号每日 Token 额度 + 需手动（官方客户端领取）+ 领取/生效时段", () => {
    const w = welfareOf(group("zcode", false, [account(0, 300_000_000)]));
    expect(w.label).toBe("单账号 +3亿 Token/日");
    expect(w.mode).toBe("manual");
    // 时段说明优先展示
    expect(w.note).toBe("19:00 开放领取 · 23:00-次日9:00 生效");
    expect(w.title).toContain("需手动领取（官方客户端领取）");
    expect(w.title).toContain("19:00 开放领取");
    expect(w.title).toContain("23:00-次日9:00 生效");
  });

  it("妙手：单账号一次性奖励，无每日签到", () => {
    const w = welfareOf(group("miaoshou", false, [account(0)]));
    expect(w.label).toBe("单账号一次性奖励");
    expect(w.mode).toBe("manual");
    expect(w.note).toBe("注册奖励");
    expect(w.title).toContain("一次性");
    expect(w.title).toContain("无每日签到");
  });

  it("数量未知（状态未加载/活动未开放）：只报节奏不给数", () => {
    const w = welfareOf(group("qoder", true, [account(null)]));
    expect(w.label).toBe("单账号每日福利");
    expect(w.mode).toBe("auto");
    expect(w.note).toBe("10:00 后开放 · 06:00-08:00");
    expect(w.title).toContain("数量未知");
  });

  it("部分账号未加载：只按已加载的报，合计标注比例", () => {
    const w = welfareOf(group("minimax", true, [account(1000), account(null)]));
    expect(w.label).toBe("单账号 +1,000 积分/日");
    expect(w.title).toContain("已加载 1/2");
    expect(w.title).toContain("合计 1,000 积分");
  });

  it("未知平台按 supportsClaim 推断领取方式", () => {
    expect(welfareOf(group("future", true, [])).mode).toBe("auto");
    expect(welfareOf(group("future", false, [])).mode).toBe("manual");
  });

  it("没有账号的自动平台仍显示时间窗", () => {
    const w = welfareOf(group("workbuddy", true, []));
    expect(w.label).toBe("单账号每日福利");
    expect(w.note).toBe("06:00-08:00");
  });
});
