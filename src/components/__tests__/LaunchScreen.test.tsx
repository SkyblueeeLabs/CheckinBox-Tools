import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import LaunchScreen from "../LaunchScreen";
import type { AccountView, AgentVisibility, Overview } from "../../lib/types";

function acct(
  provider: string,
  id: string,
  state: "claimable" | "claimed" | "notAvailable"
): AccountView {
  return {
    account: { id, provider, label: id, createdAt: 0, origin: "" },
    providerName: provider,
    accent: "#0a84ff",
    status: { state, points: 400, streakDays: 0, message: "", bonusPoints: 0 },
    quota: null,
    isActive: false,
    isClientLogin: false,
    projectCount: 0,
    sessionCount: 0,
    isLow: false,
    alertThreshold: 200,
    trend: [],
    supportsClaim: provider !== "zcode",
  };
}

function ov(accounts: AccountView[]): Overview {
  return {
    providers: [
      {
        id: "workbuddy", name: "WorkBuddy", accent: "#24c06a", docsUrl: "",
        supportsDiscovery: true, supportsOAuth: false, configured: true,
        hasClient: true, supportsClaim: true,
      },
      {
        id: "zcode", name: "Zcode", accent: "#111111", docsUrl: "",
        supportsDiscovery: false, supportsOAuth: false, configured: true,
        hasClient: true, supportsClaim: false,
      },
      {
        id: "minimax", name: "MiniMax Code", accent: "#0a84ff", docsUrl: "",
        supportsDiscovery: false, supportsOAuth: true, configured: true,
        hasClient: true, supportsClaim: true,
      },
    ],
    accounts,
    todayPoints: 400,
    claimedCount: accounts.filter((a) => a.status?.state === "claimed").length,
    totalCount: accounts.length,
    schedulerEnabled: true,
    syncEnabled: true,
    agents: {},
    plans: {},
    dataDir: "",
  };
}

function props(over: Partial<Parameters<typeof LaunchScreen>[0]> = {}) {
  return {
    visibility: {} as Record<string, AgentVisibility>,
    onClaimAll: vi.fn(),
    onClaimProvider: vi.fn(),
    onOpenProvider: vi.fn(),
    onEnter: vi.fn(),
    onRetry: vi.fn(),
    ...over,
  };
}

describe("LaunchScreen（赛博鸡蛋孵化舱）", () => {
  it("加载态：主题标题、骨架蛋与数蛋文案，可直接进入", () => {
    const p = props();
    render(<LaunchScreen overview={null} loadError={null} busy={false} {...p} />);
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe(
      "今日的赛博鸡蛋，领了嘛？"
    );
    expect(screen.getByText("正在数蛋…")).toBeInTheDocument();
    expect(document.querySelectorAll(".egg-pod.is-loading")).toHaveLength(5);
    fireEvent.click(screen.getByRole("button", { name: /进入 CheckinBox/ }));
    expect(p.onEnter).toHaveBeenCalledOnce();
  });

  it("蛋状态：可领=领按钮、需手动=去领取、已领=裂壳计数，统计条实时", () => {
    const p = props();
    render(
      <LaunchScreen
        overview={ov([
          acct("workbuddy", "w1", "claimable"),
          acct("minimax", "m1", "claimed"),
          acct("zcode", "z1", "claimable"),
        ])}
        loadError={null}
        busy={false}
        {...p}
      />
    );
    expect(screen.getByText("1 枚待领")).toBeInTheDocument();
    expect(screen.getByText("1 枚待领取")).toBeInTheDocument();
    expect(screen.getByText("已领 1/1")).toBeInTheDocument();
    expect(screen.getByText("/ 3 枚已领")).toBeInTheDocument();
    expect(screen.getByText("+400")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "领" }));
    expect(p.onClaimProvider).toHaveBeenCalledWith("workbuddy");
    fireEvent.click(screen.getByRole("button", { name: "去领取" }));
    expect(p.onOpenProvider).toHaveBeenCalledWith("zcode");
  });

  it("全部已领：主按钮禁用为「今日蛋已领完」，文案报满员", () => {
    const p = props();
    render(
      <LaunchScreen overview={ov([acct("workbuddy", "w1", "claimed")])} loadError={null} busy={false} {...p} />
    );
    expect(screen.getByRole("button", { name: "今日蛋已领完" })).toBeDisabled();
    expect(screen.getByText("领完了！今日孵化舱满员运转。")).toBeInTheDocument();
  });

  it("有可领蛋：一键领蛋回调；孵化中禁用并摇蛋", () => {
    const p = props();
    const overview = ov([acct("workbuddy", "w1", "claimable")]);
    const { rerender } = render(
      <LaunchScreen overview={overview} loadError={null} busy={false} {...p} />
    );
    fireEvent.click(screen.getByRole("button", { name: "一键领蛋" }));
    expect(p.onClaimAll).toHaveBeenCalledOnce();

    rerender(<LaunchScreen overview={overview} loadError={null} busy={true} {...p} />);
    expect(screen.getByRole("button", { name: "孵化中…" })).toBeDisabled();
    expect(document.querySelector(".egg-pod.is-hatching")).toBeInTheDocument();
  });

  it("加载失败：重试回调；Esc 直接进入", () => {
    const p = props();
    render(<LaunchScreen overview={null} loadError="网络断开" busy={false} {...p} />);
    expect(screen.getByRole("alert")).toHaveTextContent("账号与积分加载失败");
    fireEvent.click(screen.getByRole("button", { name: "重试" }));
    expect(p.onRetry).toHaveBeenCalledOnce();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(p.onEnter).toHaveBeenCalledOnce();
  });

  it("launch 隐藏的平台：蛋不上封面，统计保持全量口径", () => {
    const p = props({
      visibility: { zcode: { sidebar: true, home: true, launch: false } },
    });
    render(
      <LaunchScreen
        overview={ov([
          acct("workbuddy", "w1", "claimable"),
          acct("minimax", "m1", "claimed"),
          acct("zcode", "z1", "claimable"),
        ])}
        loadError={null}
        busy={false}
        {...p}
      />
    );
    expect(screen.queryByText("1 枚待领取")).not.toBeInTheDocument();
    expect(screen.getByText("1 枚待领")).toBeInTheDocument();
    expect(screen.getByText("/ 3 枚已领")).toBeInTheDocument();
    expect(document.querySelectorAll(".egg-pod")).toHaveLength(2);
  });

  it("全部 launch 隐藏：封面空态提示，无蛋舱", () => {
    const hide = { sidebar: true, home: true, launch: false };
    const p = props({
      visibility: { workbuddy: hide, minimax: hide, zcode: hide },
    });
    render(
      <LaunchScreen overview={ov([acct("workbuddy", "w1", "claimed")])} loadError={null} busy={false} {...p} />
    );
    expect(screen.getByText(/所有平台的蛋已在封面隐藏/)).toBeInTheDocument();
    expect(document.querySelectorAll(".egg-pod")).toHaveLength(0);
  });

  it("浮层模式：返回按钮与 Esc 回原页面，不显示进入按钮", () => {
    const onBack = vi.fn();
    const p = props({ onBack });
    render(
      <LaunchScreen overview={ov([acct("workbuddy", "w1", "claimed")])} loadError={null} busy={false} {...p} />
    );
    expect(screen.queryByRole("button", { name: /进入 CheckinBox/ })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /返回原页面/ }));
    expect(onBack).toHaveBeenCalledOnce();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onBack).toHaveBeenCalledTimes(2);
  });
});