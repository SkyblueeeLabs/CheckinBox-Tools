import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import DashboardPanel from "../DashboardPanel";
import { buildGroups } from "../../lib/groups";
import type { AccountView, Overview } from "../../lib/types";

const DAY = 86_400_000;

function account(id: string, daysLeft: number, remaining: number): AccountView {
  const expireAtMs = Date.now() + daysLeft * DAY;
  return {
    account: { id, provider: "qoder", label: id, createdAt: 0, origin: "" },
    providerName: "Qoder CN",
    accent: "#6a5acd",
    status: null,
    quota: {
      totalGranted: remaining,
      totalConsumed: 0,
      totalRemaining: remaining,
      items: [
        {
          creditType: "free",
          granted: remaining,
          consumed: 0,
          remaining,
          grantedAtMs: 0,
          expireAtMs,
        },
      ],
      message: "",
    },
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

const overview: Overview = {
  providers: [
    {
      id: "qoder",
      name: "Qoder CN",
      accent: "#6a5acd",
      docsUrl: "",
      supportsDiscovery: false,
      supportsOAuth: true,
      configured: true,
      hasClient: true,
      supportsClaim: true,
    },
  ],
  accounts: [account("晚到期", 5, 500), account("先到期", 1, 100)],
  todayPoints: 0,
  claimedCount: 0,
  totalCount: 2,
  schedulerEnabled: true,
  syncEnabled: true,
  agents: {},
  plans: {},
  dataDir: "C:\\data",
};

describe("DashboardPanel（7 天内到期）", () => {
  it("按紧迫度展示到期账号", () => {
    render(
      <DashboardPanel
        overview={overview}
        groups={buildGroups(overview)}
        visibility={{}}
        onOpenAgent={vi.fn()}
      />
    );

    expect(screen.getByText("7 天内到期")).toBeInTheDocument();
    const rows = [...document.querySelectorAll(".dash-expiring-row")];
    expect(rows).toHaveLength(2);
    expect(rows[0].textContent).toContain("先到期");
    expect(rows[1].textContent).toContain("晚到期");
  });

  it("agent 卡片展示福利画像：每日数量 / 自动或手动 / 时间窗", () => {
    render(
      <DashboardPanel
        overview={overview}
        groups={buildGroups(overview)}
        visibility={{}}
        onOpenAgent={vi.fn()}
      />
    );

    // qoder 状态未加载（points=0）→ 只报节奏，不编数量
    const card = document.querySelector(".dash-agent")!;
    expect(card.querySelector(".welfare-amount")?.textContent).toBe("单账号每日福利");
    expect(card.querySelector(".welfare-mode")?.textContent).toBe("自动");
    expect(card.querySelector(".welfare-note")?.textContent).toBe(
      "10:00 后开放 · 06:00-08:00"
    );
    // 悬浮说明里有完整信息
    expect(card.querySelector(".dash-agent-welfare")?.getAttribute("title")).toContain(
      "数量未知"
    );
  });

  it("首页隐藏的平台：卡片与到期列表不显示，全局指标保持全量口径", () => {
    render(
      <DashboardPanel
        overview={overview}
        groups={buildGroups(overview)}
        visibility={{ qoder: { sidebar: true, home: false, launch: true } }}
        onOpenAgent={vi.fn()}
      />
    );
    expect(document.querySelector(".dash-agent")).toBeNull();
    expect(screen.queryByText("7 天内到期")).not.toBeInTheDocument();
    expect(screen.getByText(/所有平台已在首页隐藏/)).toBeInTheDocument();
    // 汇总仍含被隐藏平台（500+100=600），与顶栏口径一致
    expect(document.querySelectorAll(".dash-stat-value")[0].textContent).toBe("600");
  });
});
