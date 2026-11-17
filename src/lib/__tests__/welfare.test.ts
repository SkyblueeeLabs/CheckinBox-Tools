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
    expect(w.benefits[0].label).toBe("单账号 +400~1,000 积分/日");
    expect(w.benefits[0].mode).toBe("auto");
    expect(w.benefits[0].modeText).toBe("自动领取");
    expect(w.note).toBe("06:00-08:00");
    expect(w.title).toContain("合计 1,400 积分");
    expect(w.title).toContain("06:00-08:00");
  });

  it("单账号金额一致时只报一个数", () => {
    const w = welfareOf(group("qoder", true, [account(100), account(100)]));
    expect(w.benefits[0].label).toBe("单账号 +100 Credits/日");
    expect(w.benefits[0].mode).toBe("auto");
    expect(w.note).toBe("10:00 后开放 · 06:00-08:00");
    expect(w.title).toContain("合计 200 Credits");
  });

  it("单账号时合计不重复显示", () => {
    const w = welfareOf(group("qoder", true, [account(100)]));
    expect(w.title).toContain("单账号 +100 Credits/日");
    expect(w.title).not.toContain("合计");
  });

  it("已领取也要报数（qoder 已领取时 status.points 带当天积分）", () => {
    const w = welfareOf(group("qoder", true, [account(100), account(100)]));
    expect(w.benefits[0].label).toBe("单账号 +100 Credits/日");
  });

  it("Zcode：单账号每日 Token 额度 + 需手动（官方客户端领取）+ 领取/生效时段", () => {
    const w = welfareOf(group("zcode", false, [account(0, 300_000_000)]));
    expect(w.benefits[0].label).toBe("单账号 +3亿 Token/日");
    expect(w.benefits[0].mode).toBe("manual");
    expect(w.benefits[0].modeText).toBe("手动领取");
    // 时段说明优先展示
    expect(w.note).toBe("19:00 开放领取 · 23:00-次日9:00 生效");
    expect(w.title).toContain("手动领取");
    expect(w.title).toContain("19:00 开放领取");
    expect(w.title).toContain("23:00-次日9:00 生效");
  });

  it("妙手：注册送 1,000 + 每月 +200，统一自动到账，无每日签到", () => {
    const w = welfareOf(group("miaoshou", false, [account(0)]));
    expect(w.benefits.map((b) => b.kind)).toEqual(["signup", "monthly"]);
    expect(w.benefits[0].label).toBe("注册送 1,000 积分");
    expect(w.benefits[0].modeText).toBe("自动到账");
    expect(w.benefits[1].label).toBe("每月 +200 积分");
    expect(w.benefits[1].modeText).toBe("自动到账");
    expect(w.note).toBe("");
    expect(w.title).toContain("自动到账");
    expect(w.title).toContain("无每日签到");
    expect(w.title).toContain("限时免费 · 以官方客户端为准");
  });

  it("MiniMax：按上游 7 天面板说明第 4/7 天加成，每日按基础档报数", () => {
    const days: CheckinDay[] = [400, 400, 400, 1000, 400, 400, 1000].map((points, i) => ({
      dayNo: i + 1,
      isToday: i === 5,
      points,
      bonusPoints: 0,
      claimed: i < 6,
    }));
    // 今天（第 6 天）实际是 400；福利条按基础档 400 报，加成单独成条
    const w = welfareOf(group("minimax", true, [account(400, 0, days)]));
    expect(w.benefits[0].label).toBe("单账号 +400 积分/日");
    expect(w.benefits[1].kind).toBe("bonus");
    expect(w.benefits[1].label).toBe("第 4/7 天 +1,000");
    expect(w.note).toBe("06:00-08:00");
    expect(w.title).toContain("第 4/7 天 +1,000");
  });

  it("WorkBuddy：每日签到 100，注册 2,000 / 每月 500 统一自动到账", () => {
    const w = welfareOf(group("workbuddy", true, [account(100)]));
    expect(w.benefits.map((b) => b.kind)).toEqual(["daily", "signup", "monthly"]);
    expect(w.benefits[0].label).toBe("单账号 +100 积分/日");
    expect(w.benefits[0].modeText).toBe("自动领取");
    expect(w.benefits[1].label).toBe("注册送 2,000 积分");
    expect(w.benefits[1].modeText).toBe("自动到账");
    expect(w.benefits[2].label).toBe("每月 +500 积分");
    expect(w.benefits[2].modeText).toBe("自动到账");
    expect(w.note).toBe("06:00-08:00");
    expect(w.title).toContain("注册送 2,000 积分（自动到账）");
  });

  it("数量未知（状态未加载/活动未开放）：只报节奏不给数", () => {
    const w = welfareOf(group("qoder", true, [account(null)]));
    expect(w.benefits[0].label).toBe("单账号每日福利");
    expect(w.benefits[0].mode).toBe("auto");
    expect(w.note).toBe("10:00 后开放 · 06:00-08:00");
    expect(w.title).toContain("数量未知");
  });

  it("部分账号未加载：只按已加载的报，合计标注比例", () => {
    const w = welfareOf(group("minimax", true, [account(1000), account(null)]));
    expect(w.benefits[0].label).toBe("单账号 +1,000 积分/日");
    expect(w.title).toContain("已加载 1/2");
    expect(w.title).toContain("合计 1,000 积分");
  });

  it("未知平台按 supportsClaim 推断领取方式", () => {
    expect(welfareOf(group("future", true, [])).benefits[0].mode).toBe("auto");
    expect(welfareOf(group("future", false, [])).benefits[0].mode).toBe("manual");
  });

  it("没有账号的自动平台仍显示每日条目与时间窗", () => {
    const w = welfareOf(group("workbuddy", true, []));
    expect(w.benefits[0].label).toBe("单账号每日福利");
    expect(w.note).toBe("06:00-08:00");
    // 注册 / 每月是平台规则，不依赖账号是否加载
    expect(w.benefits.map((b) => b.kind)).toContain("signup");
    expect(w.benefits.map((b) => b.kind)).toContain("monthly");
  });

  it("免费模型核对信息：首页紧凑提示 + 悬浮完整说明", () => {
    const ms = welfareOf(group("miaoshou", false, [account(0)]));
    expect(ms.title).toContain("免费模型：限时免费 · 以官方客户端为准");
    expect(ms.freeText).toBe("免费模型 限时免费 · 以官方客户端为准");
    const mm = welfareOf(group("minimax", true, [account(100)]));
    expect(mm.title).toContain("暂无免费模型");
    expect(mm.freeText).toBe("");
    const wb = welfareOf(group("workbuddy", true, [account(100)]));
    expect(wb.title).toContain("有免费模型（清单实时核对）");
    expect(wb.freeText).toBe("免费模型 实时核对");
    const zc = welfareOf(group("zcode", false, [account(0, 300_000_000)]));
    expect(zc.title).toContain("暂无免费模型");
    expect(zc.freeText).toBe("");
  });

  it("开放领取实时状态：仅手动领取平台提示；自动领取/无开放时间为 null", () => {
    const at = (h: number) => new Date(2026, 10, 16, h, 0, 0);
    expect(welfareOpenState("zcode", at(18))).toEqual({ open: false, label: "19:00 开放" });
    expect(welfareOpenState("zcode", at(19))).toEqual({ open: true, label: "现在可领" });
    expect(welfareOpenState("zcode", at(23))).toEqual({ open: true, label: "现在可领" });
    // Qoder 由工具自动领取：时间到了不等于还没领，不显示「现在可领」
    expect(welfareOpenState("qoder", at(9))).toBeNull();
    expect(welfareOpenState("qoder", at(10))).toBeNull();
    expect(welfareOpenState("minimax", at(12))).toBeNull();
    expect(welfareOpenState("workbuddy", at(12))).toBeNull();
  });
});
