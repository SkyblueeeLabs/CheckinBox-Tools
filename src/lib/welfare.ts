import type { AgentGroup } from "./types";
import { fmtCompact, fmtPoints, formatMinute } from "./api";

/**
 * 各平台「福利」画像（平台面板 / 首页共用同一份条目）。
 *
 * 统一标准：
 *   1. 条目只有四类，固定顺序：每日签到 → 注册送 → 每月 → 周期加成；
 *   2. 领取方式固定：每日 = 自动领取 / 手动领取；注册、每月 = 自动到账；
 *      周期加成跟随签到，不单独标方式；
 *   3. 数量优先取上游（签到状态 / 额度 / 周期面板）；注册 / 每月这类平台规则
 *      只写在 WELFARE_META 里，避免两处各写一份；
 *   4. 免费模型单独一行：面板给完整清单，首页给紧凑提示。
 */

export type WelfareBenefitKind = "daily" | "signup" | "monthly" | "bonus";
export type BenefitMode = "auto" | "manual" | "credit";

export interface WelfareMeta {
  /** 数量单位（积分 / Credits / Token） */
  unit: string;
  /** 每日签到方式；没有每日签到的平台不填 */
  dailyMode?: "auto" | "manual";
  /** 每日签到的平台说明（开放时间等） */
  dailyNote?: string;
  /** 领取/生效时段（如 Zcode 的 19:00 开放、23:00 生效） */
  timing?: string;
  /** 每日开放领取的小时（UTC+8），仅手动领取平台用于实时提示 */
  openHour?: number;
  /** 注册送积分（平台规则） */
  signup?: number;
  /** 每月固定发放积分（平台规则） */
  monthly?: number;
}

/** 各平台福利元数据（不含实时数量；改文案只改这里） */
export const WELFARE_META: Record<string, WelfareMeta> = {
  minimax: { unit: "积分", dailyMode: "auto" },
  qoder: { unit: "Credits", dailyMode: "auto", dailyNote: "10:00 后开放" },
  workbuddy: { unit: "积分", dailyMode: "auto", signup: 2000, monthly: 500 },
  zcode: {
    unit: "Token",
    dailyMode: "manual",
    dailyNote: "官方客户端领取",
    timing: "19:00 开放领取 · 23:00-次日9:00 生效",
    openHour: 19,
  },
  miaoshou: { unit: "积分", signup: 1000, monthly: 200 },
};

/** 条目类型的固定文案（面板标签 / 首页不重复出现） */
export const WELFARE_KIND_TEXT: Record<WelfareBenefitKind, string> = {
  daily: "每日",
  signup: "注册",
  monthly: "每月",
  bonus: "加成",
};

/** 免费模型核对信息（时效敏感，标注核对月份）。积分/额度福利见 WELFARE_META。 */
export interface FreeModelInfo {
  /** 当前确认免费的模型 */
  models?: string[];
  /** current=有免费模型；none=暂无免费模型 */
  status: "current" | "none";
  /** 清单由实时接口核对（WorkBuddy 模型目录） */
  live?: boolean;
  note?: string;
  asOf?: string;
}

export const FREE_STATUS_TEXT: Record<FreeModelInfo["status"], string> = {
  current: "有免费模型",
  none: "暂无免费模型",
};

/** 各平台免费模型核对表（改内容只改这里）。积分/额度统一走 WELFARE_META。 */
export const FREE_MODELS: Record<string, FreeModelInfo> = {
  // 实时清单由 fetch_workbuddy_free_models 从模型目录核对；此处为回退标注。
  workbuddy: { status: "current", live: true, asOf: "2026-11-16" },
  miaoshou: {
    status: "current",
    // 模型目录走 SSO + 客户端本地 IPC，工具拿不到实时清单；不写死具体模型，
    // 避免限时免费结束后还继续宣称免费。
    note: "限时免费 · 以官方客户端为准",
    asOf: "2026-11-17",
  },
  zcode: { status: "none", asOf: "2026-11-16" },
  minimax: { status: "none", asOf: "2026-11-16" },
  qoder: { status: "none", asOf: "2026-11-16" },
};

/** 每日开放领取的实时状态（现在可领 / 未到点）。
 *
 * 只给**需要手动领取**的平台用（Zcode）：自动领取的平台由工具在时间窗内处理，
 * 时间到了不代表还没领，再显示「现在可领」会误报；其余平台返回 null。
 */
export function welfareOpenState(
  providerId: string,
  now: Date = new Date()
): { open: boolean; label: string } | null {
  const meta = WELFARE_META[providerId];
  const hour = meta?.openHour;
  if (hour === undefined || meta?.dailyMode !== "manual") return null;
  const open = now.getHours() >= hour;
  return { open, label: open ? "现在可领" : `${hour}:00 开放` };
}

/** 一条福利：类型 + 文案 + 领取方式 */
export interface WelfareBenefit {
  kind: WelfareBenefitKind;
  /** 主文案（含数量），如「单账号 +100 积分/日」「注册送 2,000 积分」 */
  label: string;
  /** 领取方式；周期加成跟随签到，不单独标 */
  mode?: BenefitMode;
  /** 方式徽标文案：自动领取 / 手动领取 / 自动到账 */
  modeText?: string;
}

export interface WelfareView {
  /** 固定顺序的福利条目 */
  benefits: WelfareBenefit[];
  /** 每日签到的时段 / 开放说明 */
  note: string;
  /** 免费模型的紧凑提示（首页用；没有免费模型的平台为空） */
  freeText: string;
  /** 悬浮完整说明 */
  title: string;
}

/** 计算某个 agent 的福利概览（面板 / 首页共用） */
export function welfareOf(group: AgentGroup): WelfareView {
  const meta: WelfareMeta = WELFARE_META[group.provider.id] ?? {
    unit: "积分",
    dailyMode: group.provider.supportsClaim ? "auto" : "manual",
  };

  const fmt = (n: number) => (meta.unit === "Token" ? fmtCompact(n) : fmtPoints(n));
  const benefits: WelfareBenefit[] = [];
  const parts: string[] = [];
  let note = "";

  // 1) 每日签到
  if (meta.dailyMode) {
    // 时间窗（自动领取的执行区间）与平台说明并列，如「10:00 后开放 · 06:00-08:00」
    const window =
      meta.dailyMode === "auto"
        ? `${formatMinute(group.schedule.startMinute)}-${formatMinute(
            group.schedule.endMinute
          )}`
        : "";
    // 时段说明（Zcode 的 19:00 开放 / 23:00 生效）优先于普通说明
    note = meta.timing ?? [meta.dailyNote, window].filter(Boolean).join(" · ");
    const modeText = meta.dailyMode === "manual" ? "手动领取" : "自动领取";

    // 上游给的周期面板（MiniMax 7 天）：第 4/7 天 1,000、其余 400。
    // 只展示上游数据，不在前端硬编码天数规则。
    const cycle = group.accounts
      .map((a) => a.status?.days)
      .find((days) => days && days.length > 0);
    const cycleBase = cycle?.length ? Math.min(...cycle.map((d) => d.points)) : null;
    const cycleBonus = cycle?.filter((d) => d.points > (cycleBase ?? 0)) ?? [];

    // **单账号**数量：各账号今日积分（Zcode 为每日 Token 额度），0 = 未加载/未开放。
    const amounts = group.accounts.map((a) => {
      if (group.provider.id === "zcode") return a.quota?.totalGranted ?? 0;
      // 有周期面板时按基础档报数（加成单独成条，不把加成说成每天都有）
      const days = a.status?.days;
      if (days && days.length > 0) return Math.min(...days.map((d) => d.points));
      return a.status?.points ?? 0;
    });
    const known = amounts.filter((v) => v > 0);

    let label = "单账号每日福利";
    let totalPart = "";
    if (known.length > 0) {
      const min = Math.min(...known);
      const max = Math.max(...known);
      const rangeText = min === max ? fmt(min) : `${fmt(min)}~${fmt(max)}`;
      label = `单账号 +${rangeText} ${meta.unit}/日`;
      const total = known.reduce((s, v) => s + v, 0);
      totalPart =
        amounts.length > 1
          ? ` · ${amounts.length} 个账号${
              known.length < amounts.length
                ? `（已加载 ${known.length}/${amounts.length}）`
                : ""
            }合计 ${fmt(total)} ${meta.unit}`
          : "";
    }

    benefits.push({ kind: "daily", label, mode: meta.dailyMode, modeText });
    parts.push(
      `${known.length > 0 ? label + totalPart : "单账号每日福利数量未知"} · ${modeText}${
        note ? ` · ${note}` : ""
      }`
    );

    // 周期加成（上游面板里的高额天），固定排在每日之后
    if (cycleBonus.length > 0) {
      const bonusLabel = `第 ${cycleBonus.map((d) => d.dayNo).join("/")} 天 +${[
        ...new Set(cycleBonus.map((d) => d.points)),
      ]
        .sort((a, b) => a - b)
        .map(fmt)
        .join("/")}`;
      benefits.push({ kind: "bonus", label: bonusLabel });
      parts.push(bonusLabel);
    }
  }

  // 2) 注册送（平台规则，自动到账）
  if (meta.signup !== undefined) {
    const label = `注册送 ${fmt(meta.signup)} ${meta.unit}`;
    benefits.push({ kind: "signup", label, mode: "credit", modeText: "自动到账" });
    parts.push(`${label}（自动到账）`);
  }

  // 3) 每月（平台规则，自动到账）
  if (meta.monthly !== undefined) {
    const label = `每月 +${fmt(meta.monthly)} ${meta.unit}`;
    benefits.push({ kind: "monthly", label, mode: "credit", modeText: "自动到账" });
    parts.push(`${label}（自动到账）`);
  }

  if (!meta.dailyMode) parts.push("无每日签到");

  // 4) 免费模型（单独一行；首页只给紧凑提示，完整清单在悬浮说明里）
  const free = FREE_MODELS[group.provider.id];
  const freeText = free
    ? free.models?.length
      ? `免费模型 ${free.models.join("、")}`
      : free.live
        ? "免费模型 实时核对"
        : free.note
          ? `免费模型 ${free.note}`
          : ""
    : "";
  const freeSuffix = free
    ? free.models?.length
      ? ` · 免费模型：${free.models.join("、")}`
      : free.live
        ? " · 有免费模型（清单实时核对）"
        : free.note
          ? ` · 免费模型：${free.note}`
          : ` · ${FREE_STATUS_TEXT[free.status]}`
    : "";

  return {
    benefits,
    note,
    freeText,
    title: `${parts.length > 0 ? parts.join(" · ") : "暂无福利"}${freeSuffix}`,
  };
}
