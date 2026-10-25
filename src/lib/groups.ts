import type { AccountView, AgentGroup, Overview } from "./types";

/**
 * 账号在分组内的排序：客户端登录中的账号最靠前（它就是这个客户端的「现场」），
 * 其次是工具里的当前账号，然后按可签 → 已签 → 其他。
 * 同级保持原有顺序（稳定排序）。
 */
function accountWeight(a: AccountView): number {
  if (a.isClientLogin) return 0;
  if (a.isActive) return 1;
  const state = a.status?.state;
  if (state === "claimable") return 2;
  if (state === "claimed") return 3;
  return 4;
}

/** 把 overview 折成侧边栏/面板要用的分组 */
export function buildGroups(overview: Overview): AgentGroup[] {
  return overview.providers.map((provider) => {
    const accounts = [...overview.accounts.filter((a) => a.account.provider === provider.id)]
      .sort((x, y) => accountWeight(x) - accountWeight(y));
    return {
      provider,
      accounts,
      totalRemaining: accounts.reduce((s, a) => s + (a.quota?.totalRemaining ?? 0), 0),
      totalGranted: accounts.reduce((s, a) => s + (a.quota?.totalGranted ?? 0), 0),
      claimableCount: accounts.filter((a) => a.status?.state === "claimable").length,
      claimedToday: accounts.filter((a) => a.status?.state === "claimed").length,
      schedule: {
        ...(overview.agents[provider.id] ?? {
          enabled: true,
          startMinute: 6 * 60,
          endMinute: 8 * 60,
          accountCount: accounts.length,
          todayClaimed: 0,
          todayClaimable: 0,
          refreshIntervalMinutes: 10,
        }),
        alertThreshold: overview.accounts
          .find((a) => a.account.provider === provider.id)
          ?.alertThreshold,
      },
    };
  });
}
