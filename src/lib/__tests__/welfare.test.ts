import { describe, expect, it } from "vitest";
import { welfareOf, welfareOpenState } from "../welfare";
import type { AccountView, AgentGroup, CheckinDay, CheckinStatus, ProviderView, Quota } from "../types";

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

function account(points: number | null, granted = 0, days?: CheckinDay[]): AccountView {
  const status: CheckinStatus | null =
    points === null
      ? null
      : {
          state: points > 0 ? "claimed" : "claimable",
          points,
          streakDays: 0,
          message: "",
          bonusPoints: 0,
          days,
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

  it("妙手：注册送 1,000，之后每月 +200，自动发放（无需领取）", () => {
    const w = welfareOf(group("miaoshou", false, [account(0)]));
    expect(w.label).toBe("单账号每月 +200 积分");
    expect(w.mode).toBe("auto");
    expect(w.modeText).toBe("自动发放");
    expect(w.note).toBe("注册送 1,000 积分");
    expect(w.title).toContain("自动发放");
    expect(w.title).toContain("注册送 1,000 积分");
    expect(w.title).toContain("无每日签到");
    expect(w.title).toContain("LongCat-2.5");
  });

  it("MiniMax：按上游 7 天面板说明第 4/7 天加成，每日按基础档报数", () => {
    const days: CheckinDay[] = [400, 400, 400, 1000, 400, 400, 1000].map((points, i) => ({
      dayNo: i + 1,
      isToday: i === 5,
      points,
      bonusPoints: 0,
      claimed: i < 6,
    }));
    // 今天（第 6 天）实际是 400；福利条按基础档 400 报，加成写在说明里
    const w = welfareOf(group("minimax", true, [account(400, 0, days)]));
    expect(w.label).toBe("单账号 +400 积分/日");
    expect(w.note).toBe("第 4/7 天 +1,000 · 06:00-08:00");
    expect(w.title).toContain("第 4/7 天 +1,000");
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

  it("免费模型核对信息附在悬浮说明：当前免费 / 曾有 / 以官方为准", () => {
    const ms = welfareOf(group("miaoshou", false, [account(0)]));
    expect(ms.title).toContain("免费模型：LongCat-2.5（官方免费）");
    const mm = welfareOf(group("minimax", true, [account(100)]));
    expect(mm.title).toContain("暂无免费模型");
    const wb = welfareOf(group("workbuddy", true, [account(100)]));
    expect(wb.title).toContain("有免费模型（清单实时核对）");
    const zc = welfareOf(group("zcode", false, [account(0, 300_000_000)]));
    expect(zc.title).toContain("暂无免费模型");
  });

  it("开放领取实时状态：到点=现在可领，未到= N:00 开放；无开放小时=null", () => {
    const at = (h: number) => new Date(2026, 10, 16, h, 0, 0);
    expect(welfareOpenState("zcode", at(18))).toEqual({ open: false, label: "19:00 开放" });
    expect(welfareOpenState("zcode", at(19))).toEqual({ open: true, label: "现在可领" });
    expect(welfareOpenState("zcode", at(23))).toEqual({ open: true, label: "现在可领" });
    expect(welfareOpenState("qoder", at(9))).toEqual({ open: false, label: "10:00 开放" });
    expect(welfareOpenState("qoder", at(10))).toEqual({ open: true, label: "现在可领" });
    expect(welfareOpenState("minimax", at(12))).toBeNull();
    expect(welfareOpenState("workbuddy", at(12))).toBeNull();
  });
});
