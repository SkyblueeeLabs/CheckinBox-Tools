import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import AccountCard from "../AccountCard";
import type { AccountView } from "../../lib/types";

function view(over: Partial<AccountView> = {}): AccountView {
  return {
    account: { id: "a1", provider: "minimax", label: "测试账号", createdAt: 0, origin: "本机" },
    providerName: "MiniMax Code",
    accent: "#0a84ff",
    status: null,
    quota: null,
    isActive: false,
    isClientLogin: false,
    projectCount: 3,
    sessionCount: 5,
    isLow: false,
    alertThreshold: 200,
    trend: [],
    ...over,
  };
}

const noop = vi.fn();

function renderCard(over: Partial<AccountView> = {}, switchable = true) {
  return render(
    <AccountCard
      view={view(over)}
      busy={false}
      quotaLoading={false}
      switchable={switchable}
      onSwitchClient={noop}
      onCheckin={noop}
      onRefreshQuota={noop}
      onSetActive={noop}
      onLaunch={noop}
      onRemove={noop}
    />
  );
}

/** 小卡片默认收起：点一下展开（可访问名随账号内容变化，直接按类名取） */
function expand() {
  fireEvent.click(document.querySelector(".acct-mini")!);
}

describe("AccountCard（小卡片 + 点击展开）", () => {
  it("收起态展示账号名、id 副名字（默认打码）、状态徽标和剩余积分摘要", () => {
    localStorage.removeItem("checkinbox-log-mask:minimax");
    renderCard({
      account: { id: "abcd1234efgh", provider: "minimax", label: "测试账号", createdAt: 0, origin: "本机" },
      status: { state: "claimable", points: 400, streakDays: 0, message: "可签到 +400 积分", bonusPoints: 0 },
      quota: { totalGranted: 1000, totalConsumed: 600, totalRemaining: 400, items: [], message: "" },
    });
    expect(screen.getByText("测试账号")).toBeInTheDocument();
    expect(screen.getByText("ab****gh")).toBeInTheDocument();
    expect(screen.getByText("可签到")).toBeInTheDocument();
    expect(screen.getByText("400")).toBeInTheDocument();
    expect(screen.getByText("可领 +400")).toBeInTheDocument();
  });

  it("关闭该 agent 的打码开关后，卡片副名字显示完整 id", () => {
    localStorage.setItem("checkinbox-log-mask:minimax", "0");
    renderCard({
      account: { id: "abcd1234efgh", provider: "minimax", label: "测试账号", createdAt: 0, origin: "本机" },
    });
    expect(screen.getByText("abcd1234efgh")).toBeInTheDocument();
    localStorage.removeItem("checkinbox-log-mask:minimax");
  });

  it("展开后才出现签到按钮和积分明细", () => {
    renderCard({
      status: { state: "claimable", points: 400, streakDays: 0, message: "可签到 +400 积分", bonusPoints: 0 },
    });
    expect(screen.queryByRole("button", { name: "签到" })).not.toBeInTheDocument();
    expand();
    expect(screen.getByRole("button", { name: "签到" })).toBeEnabled();
    expect(screen.getByText("本机")).toBeInTheDocument();
  });

  it("客户端登录中的账号带徽标，展开后只有「打开客户端」（它就是现场）", () => {
    renderCard({ isClientLogin: true });
    expect(screen.getByText("客户端登录")).toBeInTheDocument();
    expand();
    expect(screen.getByText("打开客户端")).toBeInTheDocument();
    expect(screen.queryByText("切换并打开客户端")).not.toBeInTheDocument();
  });

  it("非客户端登录的账号展开后是「切换并打开客户端」一条龙按钮", () => {
    renderCard({ isClientLogin: false });
    expand();
    expect(screen.getByText("切换并打开客户端")).toBeEnabled();
  });

  it("switchable=false 的平台展开后只有普通「打开客户端」", () => {
    renderCard({}, false);
    expand();
    expect(screen.queryByText("切换并打开客户端")).not.toBeInTheDocument();
    expect(screen.getByText("打开客户端")).toBeInTheDocument();
  });

  it("积分低于预警阈值时小卡片数字标红，展开后有预警提示", () => {
    renderCard({
      quota: { totalGranted: 1000, totalConsumed: 900, totalRemaining: 80, items: [], message: "" },
      isLow: true,
      alertThreshold: 200,
    });
    expect(document.querySelector(".acct-mini-remaining b.low")).toBeInTheDocument();
    expand();
    expect(screen.getByText("积分低于预警线（200）")).toBeInTheDocument();
  });

  it("状态未知时摘要提示读取中，展开后签到按钮禁用", () => {
    renderCard();
    expect(screen.getByText("正在读取…")).toBeInTheDocument();
    expand();
    expect(screen.getByText("已处理")).toBeDisabled();
  });
});
