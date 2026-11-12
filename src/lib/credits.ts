import type { CreditItem } from "./types";

/** 官方分组固定展示顺序（与后端 parse_summary 一致） */
export const CREDIT_GROUP_ORDER = ["plan_base", "plan_bonus", "benefit", "addon"] as const;

/** 分组 kind → 中文名（服务端没返回该组时用它兜底，如弹窗里的空组入口） */
export const CREDIT_GROUP_LABELS: Record<string, string> = {
  plan_base: "套餐基础积分",
  plan_bonus: "套餐赠送积分",
  benefit: "平台奖励积分",
  addon: "购买积分",
};

export function groupLabel(kind: string): string {
  return CREDIT_GROUP_LABELS[kind] ?? kind;
}

const DAY_MS = 86_400_000;

/** benefit/addon 分组里是否有 3 天内到期的剩余批次（「包含即将到期的积分」标记） */
export function groupExpiring(items: CreditItem[], kind: string, now = Date.now()): boolean {
  if (kind !== "benefit" && kind !== "addon") return false;
  const limit = now + 3 * DAY_MS;
  return items.some(
    (i) => i.creditType === kind && i.remaining > 0 && i.expireAtMs > now && i.expireAtMs <= limit
  );
}

/** 批次表格的类别列：兼容各平台的旧取值与 WorkBuddy 的分组 kind */
export function creditTypeLabel(t: string): string {
  switch (t) {
    case "purchased":
    case "addon":
      return "购买";
    case "plan":
      return "套餐";
    case "plan_base":
      return "套餐基础";
    case "plan_bonus":
      return "套餐赠送";
    case "benefit":
      return "平台奖励";
    case "free":
    case "daily":
      return "签到";
    default:
      return "其他";
  }
}