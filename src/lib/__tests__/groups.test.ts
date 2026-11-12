import { describe, expect, it } from "vitest";
import { buildGroups } from "../groups";
import type { AccountView, Overview, ProviderView } from "../types";

function provider(id: string): ProviderView {
  return {
    id,
    name: id,
    accent: "#000",
    docsUrl: "",
    supportsDiscovery: true,
    supportsOAuth: true,
    configured: true,
    hasClient: true,
    supportsClaim: true,
  };
}

function account(
  id: string,
  prov: string,
  over: Partial<AccountView> = {}
): AccountView {
  return {
    account: { id, provider: prov, label: id, createdAt: 0, origin: "" },
    providerName: prov,
    accent: "#000",
    status: null,
    quota: null,
    isActive: false,
    isClientLogin: false,
    projectCount: 0,
    sessionCount: 0,
    isLow: false,
    alertThreshold: 200,
    trend: [],
    supportsClaim: true,
    ...over,
  };
}

function overview(accounts: AccountView[], agents: Overview["agents"] = {}): Overview {
  const providerIds = [...new Set(accounts.map((a) => a.account.provider))];
  return {
    accounts,
    providers: providerIds.map(provider),
    todayPoints: 0,
    claimedCount: 0,
    totalCount: accounts.length,
    schedulerEnabled: true,
    syncEnabled: true,
    agents,
    plans: {},
    dataDir: "",
  };
}

describe("buildGroups", () => {
  it("按 provider 分组并聚合积分与签到计数", () => {
    const ov = overview([
      account("a1", "minimax", {
        quota: { totalGranted: 100, totalConsumed: 40, totalRemaining: 60, items: [], message: "" },
        status: { state: "claimable", points: 400, streakDays: 0, message: "", bonusPoints: 0 },
      }),
      account("a2", "minimax", {
        quota: { totalGranted: 50, totalConsumed: 50, totalRemaining: 0, items: [], message: "" },
        status: { state: "claimed", points: 0, streakDays: 0, message: "", bonusPoints: 0 },
      }),
      account("b1", "qoder", {
        quota: { totalGranted: 10, totalConsumed: 0, totalRemaining: 10, items: [], message: "" },
      }),
    ]);

    const groups = buildGroups(ov);
    expect(groups).toHaveLength(2);

    const minimax = groups.find((g) => g.provider.id === "minimax")!;
    expect(minimax.accounts).toHaveLength(2);
    expect(minimax.totalRemaining).toBe(60);
    expect(minimax.totalGranted).toBe(150);
    expect(minimax.claimableCount).toBe(1);
    expect(minimax.claimedToday).toBe(1);

    const qoder = groups.find((g) => g.provider.id === "qoder")!;
    expect(qoder.accounts).toHaveLength(1);
    expect(qoder.totalRemaining).toBe(10);
  });

  it("没有账号的 provider 也保留分组（空态引导添加账号）", () => {
    const ov = overview([account("a1", "minimax")], {});
    // 手动补一个没有账号的 provider
    ov.providers.push(provider("miaoshou"));
    const groups = buildGroups(ov);
    expect(groups).toHaveLength(2);
    expect(groups.find((g) => g.provider.id === "miaoshou")!.accounts).toHaveLength(0);
  });

  it("客户端登录中的账号排最前，然后是当前账号，再是可签/已签", () => {
    const ov = overview([
      account("plain", "minimax"),
      account("claimed", "minimax", {
        status: { state: "claimed", points: 0, streakDays: 0, message: "", bonusPoints: 0 },
      }),
      account("active", "minimax", { isActive: true }),
      account("client", "minimax", { isClientLogin: true }),
      account("claimable", "minimax", {
        status: { state: "claimable", points: 0, streakDays: 0, message: "", bonusPoints: 0 },
      }),
    ]);

    const order = buildGroups(ov)[0].accounts.map((a) => a.account.id);
    expect(order).toEqual(["client", "active", "claimable", "claimed", "plain"]);
  });

  it("agents 里没有配置的 provider 用默认调度值兜底", () => {
    const ov = overview([account("a1", "qoder")], {});
    const group = buildGroups(ov)[0];
    expect(group.schedule.enabled).toBe(true);
    expect(group.schedule.startMinute).toBe(6 * 60);
    expect(group.schedule.refreshIntervalMinutes).toBe(10);
  });

  it("预警阈值从该分组的第一个账号带过来", () => {
    const ov = overview([account("a1", "minimax", { alertThreshold: 888 })], {});
    expect(buildGroups(ov)[0].schedule.alertThreshold).toBe(888);
  });
});
