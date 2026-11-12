import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import CreditsDialog from "../CreditsDialog";
import { api } from "../../lib/api";
import type { CreditGroup, CreditItem, CreditPackagePage, CreditUsagePage, Quota } from "../../lib/types";

vi.mock("../../lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../lib/api")>();
  return {
    ...actual,
    api: {
      workbuddyCreditPackages: vi.fn(),
      workbuddyCreditUsage: vi.fn(),
      openExternal: vi.fn(),
    },
  };
});

const DAY = 86_400_000;
const NOW = Date.now();

function item(over: Partial<CreditItem> = {}): CreditItem {
  return {
    creditType: "benefit",
    granted: 100,
    consumed: 0,
    remaining: 100,
    grantedAtMs: 0,
    expireAtMs: NOW + 2 * DAY,
    ...over,
  };
}

function group(over: Partial<CreditGroup> = {}): CreditGroup {
  return {
    kind: "benefit",
    name: "平台奖励积分",
    total: 4443,
    used: 1137.5,
    remaining: 3305.5,
    count: 47,
    timeMs: NOW + 2 * DAY,
    usesExpiry: true,
    ...over,
  };
}

function quota(): Quota {
  return {
    totalGranted: 4943,
    totalConsumed: 1137.5,
    totalRemaining: 3805.5,
    items: [item()],
    message: "",
    creditGroups: [
      group({ kind: "plan_base", name: "套餐基础积分", total: 500, used: 0, remaining: 500, count: 1, usesExpiry: false }),
      group(),
    ],
  };
}

const validPage: CreditPackagePage = {
  items: [{ id: "r1", name: "有效包·签到奖励", total: 100, used: 20, remaining: 80, expireAtMs: NOW + 2 * DAY }],
  total: 1,
  hasMore: false,
};
const expiredPage: CreditPackagePage = {
  items: [{ id: "r2", name: "过期包·上月套餐", total: 500, used: 500, remaining: 0, expireAtMs: NOW - 5 * DAY }],
  total: 1,
  hasMore: false,
};
const usagePage: CreditUsagePage = {
  items: [
    { requestId: "q1", time: "2026-11-09 10:00:00", input: "hello world", inputTrunc: "hello…", credit: 0.86, model: "claude-sonnet" },
    { requestId: "q2", time: "2026-11-09 09:00:00", input: "ping", inputTrunc: "", credit: 2, model: "gpt-5" },
  ],
  total: 2,
};

function renderDialog(onClose = vi.fn()) {
  render(
    <CreditsDialog
      accountId="acc1"
      accountLabel="晚晚"
      providerId="workbuddy"
      providerName="WorkBuddy"
      quota={quota()}
      onClose={onClose}
    />
  );
  return onClose;
}

beforeEach(() => {
  vi.mocked(api.workbuddyCreditPackages).mockImplementation(async (_id, _kind, expired) =>
    expired ? expiredPage : validPage
  );
  vi.mocked(api.workbuddyCreditUsage).mockResolvedValue(usagePage);
  vi.mocked(api.openExternal).mockResolvedValue(undefined);
});

afterEach(() => {
  vi.clearAllMocks();
  vi.restoreAllMocks();
});

describe("CreditsDialog（WorkBuddy 积分明细弹窗）", () => {
  it("概览态：标题、账号名、分组概览与未返回分组的入口", () => {
    renderDialog();
    expect(screen.getByRole("heading", { name: "积分明细" })).toBeInTheDocument();
    expect(screen.getByText("WorkBuddy · 晚晚")).toBeInTheDocument();
    expect(screen.getByText("套餐基础积分")).toBeInTheDocument();
    expect(screen.getByText("平台奖励积分")).toBeInTheDocument();
    // summary 没返回的组也给入口（套餐赠送 / 购买）
    expect(screen.getByRole("button", { name: /套餐赠送积分/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /购买积分/ })).toBeInTheDocument();
    // 还没切到消耗明细，不该请求 usage
    expect(api.workbuddyCreditUsage).not.toHaveBeenCalled();
  });

  it("点分组「查看详情」拉取有效期内资源包，显示包名/剩余/到期徽章", async () => {
    renderDialog();
    fireEvent.click(screen.getAllByRole("button", { name: /查看详情/ })[0]);
    expect(screen.getByRole("heading", { name: "套餐基础积分" })).toBeInTheDocument();
    await waitFor(() => expect(screen.getByText("有效包·签到奖励")).toBeInTheDocument());
    expect(api.workbuddyCreditPackages).toHaveBeenCalledWith("acc1", "plan_base", false, 1);
    expect(screen.getByText("剩余 80")).toBeInTheDocument();
    expect(screen.getByText("2 天内到期")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "有效期内" })).toHaveAttribute("aria-pressed", "true");
  });

  it("切到「已过期」用 expired=true 重新拉取，过期包不显示到期徽章", async () => {
    renderDialog();
    fireEvent.click(screen.getAllByRole("button", { name: /查看详情/ })[0]);
    await waitFor(() => expect(screen.getByText("有效包·签到奖励")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "已过期" }));
    await waitFor(() => expect(screen.getByText("过期包·上月套餐")).toBeInTheDocument());
    expect(api.workbuddyCreditPackages).toHaveBeenCalledWith("acc1", "plan_base", true, 1);
    // 有效期内面板保持挂载但隐藏，其到期徽章不可见；过期包本身无徽章
    expect(screen.queryByText("2 天内到期")).not.toBeVisible();
  });

  it("hasMore 时显示「加载更多」并翻页追加", async () => {
    vi.mocked(api.workbuddyCreditPackages).mockImplementation(async (_id, _kind, _expired, page) => ({
      items: [{ id: `p${page}`, name: `第${page}页包`, total: 10, used: 0, remaining: 10, expireAtMs: 0 }],
      total: 2,
      hasMore: page === 1,
    }));
    renderDialog();
    fireEvent.click(screen.getAllByRole("button", { name: /查看详情/ })[0]);
    await waitFor(() => expect(screen.getByText("第1页包")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "加载更多" }));
    await waitFor(() => expect(screen.getByText("第2页包")).toBeInTheDocument());
    expect(api.workbuddyCreditPackages).toHaveBeenCalledWith("acc1", "plan_base", false, 2);
    // 第一页仍在（追加而非替换）
    expect(screen.getByText("第1页包")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "加载更多" })).not.toBeInTheDocument();
  });

  it("空分组显示空态文案", async () => {
    vi.mocked(api.workbuddyCreditPackages).mockResolvedValue({ items: [], total: 0, hasMore: false });
    renderDialog();
    fireEvent.click(screen.getAllByRole("button", { name: /查看详情/ })[0]);
    await waitFor(() => expect(screen.getByText("暂无资源包")).toBeInTheDocument());
  });

  it("切到「消耗明细」拉取 usage，积分两位小数，「查看更多」打开外链", async () => {
    renderDialog();
    fireEvent.click(screen.getByRole("button", { name: "消耗明细" }));
    await waitFor(() => expect(screen.getByText("claude-sonnet")).toBeInTheDocument());
    expect(api.workbuddyCreditUsage).toHaveBeenCalledWith("acc1");
    expect(screen.getByText("0.86")).toBeInTheDocument();
    expect(screen.getByText("2.00")).toBeInTheDocument();
    expect(screen.getByText("hello…")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /查看更多/ }));
    expect(api.openExternal).toHaveBeenCalledWith("https://www.workbuddy.cn/profile/plans-usage#usage-detail");
  });

  it("「返回概览」回到顶层，「关闭」/ Esc / 点遮罩都触发 onClose", async () => {
    const onClose = renderDialog();
    fireEvent.click(screen.getAllByRole("button", { name: /查看详情/ })[0]);
    await waitFor(() => expect(screen.getByText("有效包·签到奖励")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "返回概览" }));
    expect(screen.getByRole("heading", { name: "积分明细" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "关闭" }));
    expect(onClose).toHaveBeenCalledTimes(1);
    renderDialog(onClose);
    fireEvent(screen.getByRole("dialog"), new Event("cancel", { cancelable: true }));
    expect(onClose).toHaveBeenCalledTimes(2);
    renderDialog(onClose);
    fireEvent.mouseDown(screen.getByRole("dialog"));
    expect(onClose).toHaveBeenCalledTimes(3);
  });
});
