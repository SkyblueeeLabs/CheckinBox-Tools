import { fireEvent, render, screen, within } from "@testing-library/react";
import type { ComponentProps } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import AgentGroupPanel from "../AgentGroupPanel";
import { api } from "../../lib/api";
import type { AgentGroup, ProviderView } from "../../lib/types";

const noop = vi.fn();
const closedSettings = { open: false, onOpen: noop, onClose: noop, onPathChanged: noop };

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

function renderPanel(
  g: AgentGroup,
  onChatSyncChange = noop,
  extra: Partial<ComponentProps<typeof AgentGroupPanel>> = {}
) {
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
      settings={closedSettings}
      {...extra}
    />
  );
}

function renderSettings(g: AgentGroup, onChatSyncChange = noop) {
  return renderPanel(g, onChatSyncChange, { settings: { ...closedSettings, open: true } });
}

beforeEach(() => {
  vi.spyOn(api, "getClientPath").mockResolvedValue({
    path: null, configuredPath: null, source: null, detectedPath: null, error: null,
  });
});
afterEach(() => vi.restoreAllMocks());

describe("AgentGroupPanel（换号同步聊天记录开关）", () => {
  it("支持同步的 agent（WorkBuddy）在设置弹窗显示开关和说明", () => {
    renderSettings(group());
    expect(screen.getByText("换号同步聊天记录")).toBeInTheDocument();
    // WorkBuddy 的专属说明：合并语义 + MiniMax 关系说明
    expect(screen.getByText(/合并进目标账号/)).toBeInTheDocument();
    expect(screen.getByText(/MiniMax 不受此开关影响/)).toBeInTheDocument();
  });

  it("Qoder 不提供「换号同步聊天记录」开关（会话在本机共享，不搬库）", () => {
    renderSettings(
      group({
        provider: provider({ id: "qoder", name: "Qoder CN" }),
        schedule: { ...group().schedule, chatSyncSupported: false },
      })
    );
    expect(screen.queryByText("换号同步聊天记录")).not.toBeInTheDocument();
  });

  it("不支持的 agent（如 MiniMax）不渲染开关", () => {
    renderSettings(
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
    renderSettings(group(), onChange);
    const box = screen
      .getByText("换号同步聊天记录")
      .closest("label")!
      .querySelector("input")!;
    fireEvent.click(box);
    expect(onChange).toHaveBeenCalledWith("workbuddy", true);
  });

  it("开关勾选态跟随 chatSyncEnabled", () => {
    renderSettings(
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

describe("不代领的平台（ZCode）", () => {
  it("不显示「自动签到」开关，改成一句说明", () => {
    renderSettings(
      group({
        provider: provider({ id: "zcode", name: "ZCode", supportsClaim: false }),
      })
    );
    expect(screen.queryByText("自动签到")).not.toBeInTheDocument();
    expect(screen.getByText(/需在官方客户端领取/)).toBeInTheDocument();
  });

  it("面板副标题说「今日已领取」而不是「今日已签」", () => {
    renderPanel(
      group({
        provider: provider({ id: "zcode", name: "ZCode", supportsClaim: false }),
        schedule: { ...group().schedule, todayClaimed: 1 },
      })
    );
    expect(screen.getByText(/今日已领取 1 个/)).toBeInTheDocument();
  });
});

describe("AgentGroupPanel（福利统一口径：先积分/额度，后免费模型）", () => {
  it("妙手：注册送 1,000 + 每月 200 统一自动到账 + 免费模型 chip + 核对月份", () => {
    renderPanel(
      group({ provider: provider({ id: "miaoshou", name: "妙手", supportsClaim: false }) })
    );
    const brief = screen.getByRole("note", { name: "妙手 福利" });
    expect(within(brief).getByText("积分/额度")).toBeInTheDocument();
    expect(within(brief).getByText("注册")).toBeInTheDocument();
    expect(within(brief).getByText("注册送 1,000 积分")).toBeInTheDocument();
    expect(within(brief).getByText("每月")).toBeInTheDocument();
    expect(within(brief).getByText("每月 +200 积分")).toBeInTheDocument();
    expect(within(brief).getAllByText("自动到账")).toHaveLength(2);
    expect(within(brief).getByText("免费模型")).toBeInTheDocument();
    expect(within(brief).getByText("限时免费 · 以官方客户端为准")).toBeInTheDocument();
    expect(within(brief).getByText(/核对 2026-11/)).toBeInTheDocument();
  });

  it("ZCode：每日限时 Token + 开放状态 chip + 暂无免费模型", () => {
    renderPanel(
      group({ provider: provider({ id: "zcode", name: "ZCode", supportsClaim: false }) })
    );
    const brief = screen.getByRole("note", { name: "ZCode 福利" });
    expect(within(brief).getByText("每日")).toBeInTheDocument();
    expect(within(brief).getByText("手动领取")).toBeInTheDocument();
    expect(within(brief).getByText(/19:00 开放领取/)).toBeInTheDocument();
    const h = new Date().getHours();
    expect(within(brief).getByText(h >= 19 ? "现在可领" : "19:00 开放")).toBeInTheDocument();
    expect(within(brief).getByText("暂无免费模型")).toBeInTheDocument();
  });

  it("WorkBuddy 未核对：每日自动 + 注册/每月自动到账 + 免费模型待核对", () => {
    renderPanel(group(), noop, { onRefreshFree: noop });
    const brief = screen.getByRole("note", { name: "WorkBuddy 福利" });
    expect(within(brief).getByText("每日")).toBeInTheDocument();
    expect(within(brief).getByText("自动领取")).toBeInTheDocument();
    expect(within(brief).getByText("注册")).toBeInTheDocument();
    expect(within(brief).getByText("注册送 2,000 积分")).toBeInTheDocument();
    expect(within(brief).getByText("每月")).toBeInTheDocument();
    expect(within(brief).getByText("每月 +500 积分")).toBeInTheDocument();
    expect(within(brief).getAllByText("自动到账")).toHaveLength(2);
    expect(within(brief).getByText("06:00-08:00")).toBeInTheDocument();
    expect(within(brief).getByText("有免费模型 · 待实时核对")).toBeInTheDocument();
    expect(within(brief).getByRole("button", { name: "核对最新免费模型" })).toBeInTheDocument();
  });

  it("Trae CN：每日 → 注册 → 每月，单独显示暂无免费模型，支持浏览器授权", () => {
    renderPanel(
      group({
        provider: provider({
          id: "trae",
          name: "Trae CN",
          supportsClaim: true,
          supportsDiscovery: true,
          supportsOAuth: true,
          oauthReady: true,
        }),
      })
    );
    const brief = screen.getByRole("note", { name: "Trae CN 福利" });
    expect(within(brief).getByText("每日")).toBeInTheDocument();
    expect(within(brief).getByText("单账号每日福利")).toBeInTheDocument();
    expect(within(brief).getByText("自动领取")).toBeInTheDocument();
    expect(within(brief).getByText("注册送 1,500 积分")).toBeInTheDocument();
    expect(within(brief).getByText("每月 +500 积分")).toBeInTheDocument();
    expect(within(brief).queryByText(/邀好友/)).not.toBeInTheDocument();
    const modelsRow = within(brief).getByText("免费模型").parentElement!;
    expect(within(modelsRow).getByText("暂无免费模型")).toBeInTheDocument();

    expect(screen.getByRole("button", { name: "添加账号" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "从本机导入" })).toBeEnabled();
  });

  it("Qoder：自动领取平台不显示「现在可领」标签，并标明免费用户停发规则", () => {
    renderPanel(group({ provider: provider({ id: "qoder", name: "Qoder CN" }) }));
    const brief = screen.getByRole("note", { name: "Qoder CN 福利" });
    expect(within(brief).queryByText("现在可领")).not.toBeInTheDocument();
    expect(within(brief).queryByText("10:00 开放")).not.toBeInTheDocument();
    expect(within(brief).getByText(/10:00 后开放/)).toBeInTheDocument();
    expect(within(brief).getByText(/体验版（免费）用户已停发/)).toBeInTheDocument();
    expect(
      within(brief).getByRole("button", { name: "官方说明 ↗" })
    ).toBeInTheDocument();
  });

  it("minimax：每日自动免费积分 + 暂无免费模型", () => {
    renderPanel(group({ provider: provider({ id: "minimax", name: "MiniMax Code" }) }));
    const brief = screen.getByRole("note", { name: "MiniMax Code 福利" });
    expect(within(brief).getByText("自动领取")).toBeInTheDocument();
    expect(within(brief).getByText("暂无免费模型")).toBeInTheDocument();
  });

  it("WorkBuddy 实时免费模型：live chip + 实时标记 + 核对按钮回调", () => {
    const onRefreshFree = vi.fn();
    renderPanel(group(), noop, {
      liveFree: {
        models: [
          { id: "hy4", name: "Hy4", credits: "x0.00", free: true },
          { id: "pro", name: "Pro", credits: "x1", free: false },
        ],
        fetchedAt: Date.now(),
        loading: false,
        error: null,
      },
      onRefreshFree,
    });
    const brief = screen.getByRole("note", { name: "WorkBuddy 福利" });
    expect(within(brief).getByText("Hy4")).toBeInTheDocument();
    expect(within(brief).queryByText("Pro")).not.toBeInTheDocument();
    expect(within(brief).getByText(/实时/)).toBeInTheDocument();
    fireEvent.click(within(brief).getByRole("button", { name: "核对最新免费模型" }));
    expect(onRefreshFree).toHaveBeenCalledOnce();
  });

  it("实时核对不可用：保留有免费模型标注 + 失败标记", () => {
    renderPanel(group(), noop, {
      liveFree: { models: [], fetchedAt: 0, loading: false, error: "HTTP 401" },
      onRefreshFree: noop,
    });
    const brief = screen.getByRole("note", { name: "WorkBuddy 福利" });
    expect(within(brief).getByText(/实时核对不可用/)).toBeInTheDocument();
    expect(within(brief).getByText(/有免费模型/)).toBeInTheDocument();
  });

  it("Qoder：官方活动文档核对出的免费模型按 live chip 展示", () => {
    const onRefreshFree = vi.fn();
    renderPanel(
      group({ provider: provider({ id: "qoder", name: "Qoder CN" }) }),
      noop,
      {
        liveFree: {
          models: [
            { id: "hungerg-free", name: "HungerG", credits: "0.0×", free: true },
            { id: "qwen4max", name: "Qwen4-Max", credits: "0.4×", free: false },
          ],
          fetchedAt: Date.now(),
          loading: false,
          error: null,
        },
        onRefreshFree,
      }
    );
    const brief = screen.getByRole("note", { name: "Qoder CN 福利" });
    expect(within(brief).getByText("HungerG")).toBeInTheDocument();
    expect(within(brief).queryByText("Qwen4-Max")).not.toBeInTheDocument();
    expect(within(brief).getByText(/实时/)).toBeInTheDocument();
    fireEvent.click(within(brief).getByRole("button", { name: "核对最新免费模型" }));
    expect(onRefreshFree).toHaveBeenCalledOnce();
  });

  it("Qoder：活动结束后核对结果为空，显示本期暂无免费模型", () => {
    renderPanel(
      group({ provider: provider({ id: "qoder", name: "Qoder CN" }) }),
      noop,
      {
        liveFree: { models: [], fetchedAt: Date.now(), loading: false, error: null },
        onRefreshFree: noop,
      }
    );
    const brief = screen.getByRole("note", { name: "Qoder CN 福利" });
    expect(within(brief).getByText("本期暂无免费模型")).toBeInTheDocument();
  });
});
