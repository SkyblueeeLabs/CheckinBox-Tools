import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import AgentGroupPanel from "../AgentGroupPanel";
import type { AgentGroup, ProviderView } from "../../lib/types";

const noop = vi.fn();

function provider(over: Partial<ProviderView> = {}): ProviderView {
  return {
    id: "workbuddy",
    name: "WorkBuddy",
    accent: "#0052d9",
    docsUrl: "https://www.codebuddy.cn",
    supportsDiscovery: true,
    supportsOAuth: true,
    configured: true,
    hasClient: true,
    supportsClaim: true,
    ...over,
  };
}

/** AgentScheduleView 的最小可用形态（聊天同步字段是本组件关心的） */
function group(over: Partial<AgentGroup> = {}): AgentGroup {
  return {
    provider: provider(),
    accounts: [],
    totalRemaining: 0,
    totalGranted: 0,
    claimableCount: 0,
    claimedToday: 0,
    schedule: {
      enabled: true,
      startMinute: 360,
      endMinute: 480,
      accountCount: 0,
      todayClaimed: 0,
      todayClaimable: 0,
      refreshIntervalMinutes: 10,
      switchable: true,
      chatSyncEnabled: false,
      chatSyncSupported: true,
    },
    ...over,
  };
}

function renderPanel(g: AgentGroup, onChatSyncChange = noop) {
  return render(
    <AgentGroupPanel
      group={g}
      busy={false}
      quotaLoadingId={null}
      authPending={false}
      onCheckin={noop}
      onRefreshQuota={noop}
      onSetActive={noop}
      onLaunch={noop}
      onRemove={noop}
      onDiscover={noop}
      onAddAccount={noop}
      onSwitchClient={noop}
      onAlertChange={noop}
      onScheduleChange={noop}
      onChatSyncChange={onChatSyncChange}
    />
  );
}

describe("AgentGroupPanel（换号同步聊天记录开关）", () => {
  it("支持同步的 agent（WorkBuddy）在面板顶部显示开关和说明", () => {
    renderPanel(group());
    expect(screen.getByText("换号同步聊天记录")).toBeInTheDocument();
    // WorkBuddy 的专属说明：合并语义 + MiniMax 关系说明
    expect(screen.getByText(/合并进目标账号/)).toBeInTheDocument();
    expect(screen.getByText(/MiniMax 不受此开关影响/)).toBeInTheDocument();
  });

  it("Qoder 不提供「换号同步聊天记录」开关（会话在本机共享，不搬库）", () => {
    renderPanel(
      group({
        provider: provider({ id: "qoder", name: "Qoder CN" }),
        schedule: { ...group().schedule, chatSyncSupported: false },
      })
    );
    expect(screen.queryByText("换号同步聊天记录")).not.toBeInTheDocument();
  });

  it("不支持的 agent（如 MiniMax）不渲染开关", () => {
    renderPanel(
      group({
        provider: provider({ id: "minimax", name: "MiniMax Code" }),
        schedule: {
          ...group().schedule,
          chatSyncEnabled: false,
          chatSyncSupported: false,
        },
      })
    );
    expect(screen.queryByText("换号同步聊天记录")).not.toBeInTheDocument();
  });

  it("勾选/取消会回调 onChatSyncChange", () => {
    const onChange = vi.fn();
    renderPanel(group(), onChange);
    const box = screen
      .getByText("换号同步聊天记录")
      .closest("label")!
      .querySelector("input")!;
    fireEvent.click(box);
    expect(onChange).toHaveBeenCalledWith("workbuddy", true);
  });

  it("开关勾选态跟随 chatSyncEnabled", () => {
    renderPanel(
      group({
        schedule: { ...group().schedule, chatSyncEnabled: true },
      })
    );
    const box = screen
      .getByText("换号同步聊天记录")
      .closest("label")!
      .querySelector("input")! as HTMLInputElement;
    expect(box.checked).toBe(true);
  });
});

describe("不代领的平台（Zcode）", () => {
  it("不显示「自动签到」开关，改成一句说明", () => {
    renderPanel(
      group({
        provider: provider({ id: "zcode", name: "Zcode", supportsClaim: false }),
      })
    );
    expect(screen.queryByText("自动签到")).not.toBeInTheDocument();
    expect(screen.getByText(/需在官方客户端领取/)).toBeInTheDocument();
  });

  it("面板副标题说「今日已领取」而不是「今日已签」", () => {
    renderPanel(
      group({
        provider: provider({ id: "zcode", name: "Zcode", supportsClaim: false }),
        schedule: { ...group().schedule, todayClaimed: 1 },
      })
    );
    expect(screen.getByText(/今日已领取 1 个/)).toBeInTheDocument();
  });
});
