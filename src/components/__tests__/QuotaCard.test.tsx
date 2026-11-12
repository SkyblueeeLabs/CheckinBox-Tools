import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import QuotaCard from "../QuotaCard";
import type { CreditGroup, CreditItem, Quota } from "../../lib/types";

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
    used: 1137.49999907,
    remaining: 3305.50000093,
    count: 47,
    timeMs: NOW + 2 * DAY,
    usesExpiry: true,
    ...over,
  };
}

function wbQuota(over: Partial<Quota> = {}): Quota {
  return {
    totalGranted: 4943,
    totalConsumed: 1137.49999907,
    totalRemaining: 3805.50000093,
    items: [item(), item({ creditType: "plan_base", remaining: 500, expireAtMs: 0, refreshAtMs: NOW + 3 * DAY })],
    message: "剩余 3805.5 / 共 4943 credits",
    creditGroups: [
      group({ kind: "plan_base", name: "套餐基础积分", total: 500, used: 0, remaining: 500, count: 1, timeMs: NOW + 3 * DAY, usesExpiry: false }),
      group(),
    ],
    ...over,
  };
}

describe("QuotaCard（WorkBuddy 四分组概览）", () => {
  it("有官方分组时渲染分组概览，不再显示批次表格和单独到期行", () => {
    render(<QuotaCard quota={wbQuota()} providerId="workbuddy" />);
    expect(screen.getByText("积分分组")).toBeInTheDocument();
    expect(screen.getByText("套餐基础积分")).toBeInTheDocument();
    expect(screen.getByText("平台奖励积分")).toBeInTheDocument();
    // 剩余数字保留小数口径
    expect(screen.getByText("3,805.5")).toBeInTheDocument();
    expect(screen.getByText("3,305.5")).toBeInTheDocument();
    expect(screen.getByText("已使用 1,137.5/4,443")).toBeInTheDocument();
    expect(screen.getByText("剩余74%")).toBeInTheDocument();
    expect(screen.getByText("剩余100%")).toBeInTheDocument();
    // 时间线索：套餐看刷新、奖励看到期
    expect(screen.getByText(/^下次刷新/)).toBeInTheDocument();
    expect(screen.getByText(/^最近到期/)).toBeInTheDocument();
    // 批次表格被分组取代
    expect(screen.queryByText(/个发放批次/)).not.toBeInTheDocument();
  });

  it("benefit 组有 3 天内到期的剩余批次时显示即将到期标记", () => {
    render(<QuotaCard quota={wbQuota()} providerId="workbuddy" />);
    expect(screen.getByText("包含即将到期的积分")).toBeInTheDocument();
  });

  it("点分组详情时传递分组类型", () => {
    const onGroup = vi.fn();
    render(<QuotaCard quota={wbQuota()} providerId="workbuddy" onGroup={onGroup} />);
    fireEvent.click(screen.getByRole("button", { name: /查看详情 · 平台奖励积分/ }));
    expect(onGroup).toHaveBeenCalledWith("benefit");
  });

  it("summary 失败（无分组）退回批次表格，类别列按 kind 正确标注", () => {
    render(
      <QuotaCard
        quota={wbQuota({ creditGroups: [] })}
        providerId="workbuddy"
      />
    );
    expect(screen.queryByText("积分分组")).not.toBeInTheDocument();
    expect(screen.getByText(/个发放批次/)).toBeInTheDocument();
    // 旧 bug：WorkBuddy 包代码全被标成「签到」；现在按 kind 标注
    expect(screen.getByText("平台奖励")).toBeInTheDocument();
    expect(screen.getByText("套餐基础")).toBeInTheDocument();
  });

  it("其它平台不受影响：仍是批次表格 + 取整数字", () => {
    const quota: Quota = {
      totalGranted: 1000,
      totalConsumed: 600,
      totalRemaining: 400,
      items: [item({ creditType: "plan", expireAtMs: NOW + 30 * DAY })],
      message: "",
    };
    render(<QuotaCard quota={quota} providerId="minimax" />);
    expect(screen.queryByText("积分分组")).not.toBeInTheDocument();
    expect(screen.getByText("400")).toBeInTheDocument();
    expect(screen.getByText("套餐")).toBeInTheDocument();
  });
});
