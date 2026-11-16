import { fireEvent, render, screen, within } from "@testing-library/react";
import type { ComponentProps } from "react";
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
    supportsClaim: true,
    ...over,
  };
}

const noop = vi.fn();

/** 凭据失效（HTTP 401）的典型文案，来自 MiniMax provider */
const ERR =
  "凭据已失效（HTTP 401）：客户端当前登录的不是这个账号。在客户端登录此账号后重新导入，或用「切换客户端账号」切过去";

function renderCard(
  over: Partial<AccountView> = {},
  switchable = true,
  actions: Partial<ComponentProps<typeof AccountCard>> = {},
) {
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
      {...actions}
    />
  );
}

/** 展开账号诊断信息 */
function expand() {
  fireEvent.click(document.querySelector(".acct-mini")!);
}

describe("AccountCard（方块卡片与快捷操作）", () => {
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

  it("7 天内到期的积分显示到期徽标", () => {
    const expireAtMs = Date.now() + 2 * 86_400_000;
    renderCard({
      quota: {
        totalGranted: 500,
        totalConsumed: 100,
        totalRemaining: 400,
        items: [
          {
            creditType: "free",
            granted: 400,
            consumed: 0,
            remaining: 400,
            grantedAtMs: 0,
            expireAtMs,
          },
        ],
        message: "",
      },
    });
    expect(screen.getByText("2 天到期")).toBeInTheDocument();
  });

  it("Qoder 在弹窗内展示本地领取批次，即使额度接口报错", () => {
    const now = Date.now();
    renderCard({
      account: { id: "q1", provider: "qoder", label: "Qoder 账号", createdAt: 0, origin: "" },
      providerName: "Qoder CN",
      quotaError: "额度接口不可用",
      trackedGrants: [
        {
          creditType: "daily",
          granted: 100,
          consumed: 0,
          remaining: 100,
          grantedAtMs: now - 86_400_000,
          expireAtMs: now + 29 * 86_400_000,
        },
      ],
    });
    expect(screen.queryByText(/每日领取记录（本地）/)).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "积分明细" }));
    const dialog = within(screen.getByRole("dialog", { name: "积分明细" }));
    expect(dialog.getByText(/每日领取记录（本地）/)).toBeInTheDocument();
    expect(dialog.getByRole("alert")).toHaveTextContent("额度接口不可用");
    expect(dialog.getByRole("button", { name: "重试刷新" })).toBeEnabled();
  });

  it("Zcode 的额度按 Token 展示：3 亿级缩写为亿，单位标签不再是积分", () => {
    renderCard({
      account: { id: "z1", provider: "zcode", label: "Zcode 账号", createdAt: 0, origin: "本机 Zcode 登录态" },
      providerName: "Zcode",
      status: { state: "claimable", points: 0, streakDays: 0, message: "可领取", bonusPoints: 0 },
      supportsClaim: false,
      quota: {
        totalGranted: 300_000_000,
        totalConsumed: 1_109_567,
        totalRemaining: 298_890_433,
        items: [],
        message: "GLM-5.3-Flash 剩余 2.99亿 / 共 3.00亿",
      },
    });
    // 收起态：紧凑数字 + Token 标签
    expect(screen.getByText("2.99亿")).toBeInTheDocument();
    expect(screen.getByText("剩余 Token")).toBeInTheDocument();
    expect(screen.queryByText("剩余总积分")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "额度明细" }));
    const dialog = within(screen.getByRole("dialog", { name: "额度明细" }));
    expect(dialog.getByText("3亿")).toBeInTheDocument();
    expect(dialog.getByText("累计发放 Token")).toBeInTheDocument();
    expect(screen.queryByText("累计发放总积分")).not.toBeInTheDocument();
  });

  it("直接从卡片签到和设为当前", () => {
    const onCheckin = vi.fn();
    const onSetActive = vi.fn();
    renderCard({
      status: { state: "claimable", points: 400, streakDays: 0, message: "可签到 +400 积分", bonusPoints: 0 },
    }, true, { onCheckin, onSetActive });
    fireEvent.click(screen.getByRole("button", { name: "签到" }));
    fireEvent.click(screen.getByRole("button", { name: "设为当前" }));
    expect(onCheckin).toHaveBeenCalledOnce();
    expect(onSetActive).toHaveBeenCalledOnce();
    expect(screen.getByRole("button", { name: "积分明细" })).toBeEnabled();
    expect(screen.queryByText("本机")).not.toBeInTheDocument();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("卡片上的刷新按钮立即拉取最新积分", () => {
    const onRefreshQuota = vi.fn();
    renderCard({}, true, { onRefreshQuota });
    fireEvent.click(screen.getByRole("button", { name: "刷新积分" }));
    expect(onRefreshQuota).toHaveBeenCalledOnce();
  });

  it("刷新中按钮禁用并显示加载动画", () => {
    renderCard({}, true, { quotaLoading: true });
    const refresh = screen.getByRole("button", { name: "刷新积分" });
    expect(refresh).toBeDisabled();
    expect(refresh.classList.contains("is-loading")).toBe(true);
  });

  it("Zcode 卡片的刷新按钮叫「刷新额度」", () => {
    renderCard({
      account: { id: "z1", provider: "zcode", label: "Zcode 账号", createdAt: 0, origin: "本机" },
      supportsClaim: false,
    }, false);
    expect(screen.getByRole("button", { name: "刷新额度" })).toBeEnabled();
  });

  it("客户端登录中的账号可直接打开客户端", () => {
    const onLaunch = vi.fn();
    renderCard({ isClientLogin: true }, true, { onLaunch });
    expect(screen.getByText("客户端登录")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "打开客户端" }));
    expect(onLaunch).toHaveBeenCalledOnce();
    expect(screen.queryByRole("button", { name: "切换并打开客户端" })).not.toBeInTheDocument();
  });

  it("其他可切换账号可以从卡片切换并打开客户端", () => {
    const onSwitchClient = vi.fn();
    renderCard({ isClientLogin: false }, true, { onSwitchClient });
    fireEvent.click(screen.getByRole("button", { name: "切换并打开客户端" }));
    expect(onSwitchClient).toHaveBeenCalledOnce();
  });

  it("不支持切换的平台直接提供「打开客户端」", () => {
    renderCard({}, false);
    expect(screen.queryByRole("button", { name: "切换并打开客户端" })).not.toBeInTheDocument();
    expect(screen.getByText("打开客户端")).toBeInTheDocument();
  });

  it("移除账号须确认，取消时保留账号", () => {
    const onRemove = vi.fn();
    renderCard({}, true, { onRemove });
    const remove = screen.getByRole("button", { name: "移除账号" });
    fireEvent.click(remove);
    const dialog = within(screen.getByRole("dialog", { name: "移除账号？" }));
    expect(dialog.getByText(/MiniMax Code.*测试账号/)).toBeVisible();
    fireEvent.click(dialog.getByRole("button", { name: "取消" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(onRemove).not.toHaveBeenCalled();

    fireEvent.click(remove);
    fireEvent.click(dialog.getByRole("button", { name: "确认移除" }));
    expect(onRemove).toHaveBeenCalledOnce();
  });

  it("积分低于预警阈值时小卡片数字标红，明细窗口有预警提示", () => {
    renderCard({
      quota: { totalGranted: 1000, totalConsumed: 900, totalRemaining: 80, items: [], message: "" },
      isLow: true,
      alertThreshold: 200,
    });
    expect(document.querySelector(".acct-mini-remaining b.low")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "积分明细" }));
    expect(screen.getByText("积分低于预警线（200）")).toBeInTheDocument();
  });

  it("WorkBuddy 卡片直接显示小数余额和官方分组，详情在应用内另开", () => {
    renderCard({
      account: { id: "wb1", provider: "workbuddy", label: "工作账号", createdAt: 0, origin: "本机" },
      providerName: "WorkBuddy",
      quota: {
        totalGranted: 200,
        totalConsumed: 75.14,
        totalRemaining: 124.86,
        items: [],
        message: "",
        creditGroups: [
          { kind: "benefit", name: "平台奖励积分", total: 150, used: 75.14, remaining: 74.86, count: 1 },
          { kind: "plan_base", name: "套餐基础积分", total: 50, used: 0, remaining: 50, count: 1 },
        ],
      },
    });
    expect(screen.getByText("124.86")).toBeInTheDocument();
    expect(screen.getByText("74.86")).toBeInTheDocument();
    expect(screen.queryByText("已使用 75.14/150")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "积分明细" }));
    const dialog = within(screen.getByRole("dialog", { name: "积分明细" }));
    expect(dialog.getByText("WorkBuddy · 工作账号")).toBeInTheDocument();
    expect(dialog.getByText("已使用 75.14/150")).toBeInTheDocument();
    fireEvent.click(dialog.getByRole("button", { name: "关闭弹窗" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("状态未知时摘要提示读取中，签到按钮禁用", () => {
    renderCard();
    expect(screen.getByText("正在读取…")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "签到" })).toBeDisabled();
  });

  it("凭据失效时收起态显示红色徽标和异常摘要", () => {
    renderCard({ error: ERR });
    expect(screen.getByText("读取失败")).toBeInTheDocument();
    expect(screen.getByText("状态读取失败，点「重试」或重新导入")).toBeInTheDocument();
    expect(document.querySelector(".acct-card.error")).toBeInTheDocument();
  });

  it("同一个 401 错误只显示一次（积分和状态不重复），展开后按钮变为可用的「重试」", () => {
    renderCard({ error: ERR, quotaError: ERR });
    expand();
    expect(screen.getAllByText(ERR)).toHaveLength(1);
    expect(screen.getByRole("button", { name: "重试" })).toBeEnabled();
  });

  it("积分和状态报不同错误时，两条都显示", () => {
    const quotaErr = "积分接口返回 HTTP 500";
    renderCard({ error: ERR, quotaError: quotaErr });
    expand();
    expect(screen.getByText(ERR)).toBeInTheDocument();
    expect(screen.getByText(quotaErr)).toBeInTheDocument();
  });

  it("不代领的平台（Zcode）显示「待领取」和「打开Zcode领取」按钮", () => {
    renderCard({
      providerName: "Zcode",
      supportsClaim: false,
      status: {
        state: "claimable",
        points: 0,
        streakDays: 0,
        message: "当前无生效中的套餐，点「打开 Zcode 领取」去官方客户端领取",
        bonusPoints: 0,
      },
    }, false);
    expect(screen.getAllByText("待领取").length).toBeGreaterThan(0);
    expect(screen.getByRole("button", { name: "打开Zcode领取" })).toBeEnabled();
    // 不代领的平台不该出现「签到」按钮
    expect(screen.queryByRole("button", { name: "签到" })).not.toBeInTheDocument();
  });

  it("Zcode 已领取时直接打开客户端", () => {
    renderCard({
      providerName: "Zcode",
      supportsClaim: false,
      status: {
        state: "claimed",
        points: 0,
        streakDays: 0,
        message: "已领取 ZCODE GLOBAL BUILD · GLM-5.3-Flash",
        bonusPoints: 0,
      },
    }, false);
    expect(screen.getAllByText("已领取").length).toBeGreaterThan(0);
    expect(screen.queryByRole("button", { name: "打开Zcode领取" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "打开客户端" })).toBeEnabled();
  });
});
