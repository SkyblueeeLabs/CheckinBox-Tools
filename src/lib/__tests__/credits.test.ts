import { describe, expect, it } from "vitest";
import { creditTypeLabel, groupExpiring, groupLabel } from "../credits";
import { fmtCredits, fmtQuotaAmount } from "../api";
import type { CreditItem } from "../types";

function item(over: Partial<CreditItem> = {}): CreditItem {
  return {
    creditType: "benefit",
    granted: 100,
    consumed: 0,
    remaining: 100,
    grantedAtMs: 0,
    expireAtMs: 0,
    ...over,
  };
}

describe("credits（WorkBuddy 分组展示逻辑）", () => {
  it("分组 kind 映射中文名，未知 kind 原样返回", () => {
    expect(groupLabel("plan_base")).toBe("套餐基础积分");
    expect(groupLabel("plan_bonus")).toBe("套餐赠送积分");
    expect(groupLabel("benefit")).toBe("平台奖励积分");
    expect(groupLabel("addon")).toBe("购买积分");
    expect(groupLabel("whatever")).toBe("whatever");
  });

  it("类别列：旧平台取值与 WorkBuddy kind 都有正确文案", () => {
    expect(creditTypeLabel("plan")).toBe("套餐");
    expect(creditTypeLabel("purchased")).toBe("购买");
    expect(creditTypeLabel("addon")).toBe("购买");
    expect(creditTypeLabel("daily")).toBe("签到");
    expect(creditTypeLabel("free")).toBe("签到");
    expect(creditTypeLabel("plan_base")).toBe("套餐基础");
    expect(creditTypeLabel("plan_bonus")).toBe("套餐赠送");
    expect(creditTypeLabel("benefit")).toBe("平台奖励");
    expect(creditTypeLabel("other")).toBe("其他");
  });

  it("即将到期标记：只算 benefit/addon 里 3 天内到期且还有剩余的批次", () => {
    const now = 1_000_000_000_000;
    const day = 86_400_000;
    const soon = item({ expireAtMs: now + 2 * day, remaining: 10 });
    expect(groupExpiring([soon], "benefit", now)).toBe(true);
    expect(
      groupExpiring([item({ creditType: "addon", expireAtMs: now + 2 * day, remaining: 10 })], "addon", now)
    ).toBe(true);
    // 套餐组不看这个标记
    expect(groupExpiring([soon], "plan_base", now)).toBe(false);
    // 已用完 / 已过期 / 超过 3 天 / 别的分组都不算
    expect(groupExpiring([item({ expireAtMs: now + day, remaining: 0 })], "benefit", now)).toBe(false);
    expect(groupExpiring([item({ expireAtMs: now - day, remaining: 10 })], "benefit", now)).toBe(false);
    expect(groupExpiring([item({ expireAtMs: now + 4 * day, remaining: 10 })], "benefit", now)).toBe(false);
    expect(groupExpiring([item({ creditType: "addon", expireAtMs: now + day })], "benefit", now)).toBe(false);
  });
});

describe("fmtCredits / fmtQuotaAmount（WorkBuddy 小数口径）", () => {
  it("保留最多两位小数并去尾零，千分位，-0 归一", () => {
    expect(fmtCredits(3805.50000093)).toBe("3,805.5");
    expect(fmtCredits(4943)).toBe("4,943");
    expect(fmtCredits(0.86)).toBe("0.86");
    expect(fmtCredits(1137.49999907)).toBe("1,137.5");
    expect(fmtCredits(-0.001)).toBe("0");
    expect(fmtCredits(Number.NaN)).toBe("0");
  });

  it("fmtQuotaAmount 按平台路由：WorkBuddy 两位小数，其它取整，Zcode 缩写", () => {
    expect(fmtQuotaAmount(3805.50000093, "workbuddy")).toBe("3,805.5");
    expect(fmtQuotaAmount(3805.50000093, "minimax")).toBe("3,806");
    expect(fmtQuotaAmount(298890433, "zcode")).toBe("2.99亿");
  });
});