import type { AgentGroup } from "./types";
import { fmtCompact, fmtPoints, formatMinute } from "./api";

/**
 * 各平台「福利」画像（首页展示用）。
 *
 * 数量是实时的（从账号状态/额度算），节奏与领取方式是平台自身的规则：
 *   * minimax / workbuddy / qoder —— 每日签到，工具可自动领取（时间窗内错峰）
 *   * zcode —— 每日 Token 额度，只能官方客户端过验证码领取（supports_claim=false）
 *   * miaoshou —— 没有每日签到，积分来自套餐与**一次性**注册奖励
 */

/** 福利节奏：每日 / 一次性 */
export type WelfareCadence = "daily" | "oneTime";

/** 领取方式：自动 / 手动 */
export type WelfareMode = "auto" | "manual";

export interface WelfareMeta {
  cadence: WelfareCadence;
  mode: WelfareMode;
  /** 数量单位（积分 / Credits / Token） */
  unit: string;
  /** 补充说明（领取方式、开放时间等） */
  note?: string;
  /** 领取/生效时段（优先级高于 note，如 Zcode 的 19:00 开放、23:00 生效） */
  timing?: string;
  /** 每日开放领取的小时（UTC+8），用于「现在可领 / 未到点」实时提示 */
  openHour?: number;
}

/** 各平台福利元数据（不含实时数量；面板/首页共用，改文案只改这里） */
export const WELFARE_META: Record<string, WelfareMeta> = {
  minimax: { cadence: "daily", mode: "auto", unit: "积分" },
  qoder: { cadence: "daily", mode: "auto", unit: "Credits", note: "10:00 后开放", openHour: 10 },
  workbuddy: { cadence: "daily", mode: "auto", unit: "积分" },
  zcode: {
    cadence: "daily",
    mode: "manual",
    unit: "Token",
    note: "官方客户端领取",
    timing: "19:00 开放领取 · 23:00-次日9:00 生效",
    openHour: 19,
  },
  miaoshou: {
    cadence: "oneTime",
    mode: "manual",
    unit: "积分",
    note: "注册送 200 积分",
  },
};

/** 免费模型核对信息（时效敏感，标注核对月份）。积分/额度福利见 WELFARE_META。 */
export interface FreeModelInfo {
  /** 当前确认免费的模型 */
  models?: string[];
  /** 注册 / 一次性积分奖励 */
  registerBonus?: string;
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
    models: ["LongCat-2.5（官方免费）"],
    registerBonus: "注册送 200 积分",
    asOf: "2026-11-16",
  },
  zcode: { status: "none", asOf: "2026-11-16" },
  minimax: { status: "none", asOf: "2026-11-16" },
  qoder: { status: "none", asOf: "2026-11-16" },
};

/** 每日开放领取的实时状态（现在可领 / 未到点）；无开放小时的平台返回 null */
export function welfareOpenState(
  providerId: string,
  now: Date = new Date()
): { open: boolean; label: string } | null {
  const hour = WELFARE_META[providerId]?.openHour;
  if (hour === undefined) return null;
  const open = now.getHours() >= hour;
  return { open, label: open ? "现在可领" : `${hour}:00 开放` };
}

export interface WelfareView {
  /** 主文案（含数量，如「每日 +1,400 积分」） */
  label: string;
  mode: WelfareMode;
  /** 右侧补充说明（时间窗 / 领取方式） */
  note: string;
  /** 悬浮完整说明 */
  title: string;
}

/** 计算某个 agent 的福利概览（首页卡片用） */
export function welfareOf(group: AgentGroup): WelfareView {
  const meta = WELFARE_META[group.provider.id] ?? {
    cadence: "daily" as const,
    mode: (group.provider.supportsClaim ? "auto" : "manual") as WelfareMode,
    unit: "积分",
  };

  const fmt = (n: number) => (meta.unit === "Token" ? fmtCompact(n) : fmtPoints(n));

  // 时间窗（自动领取的执行区间）与平台说明并列展示，如
  // 「10:00 后开放 · 06:00-08:00」—— 前者是平台规则，后者是工具的执行窗口
  const window = meta.mode === "auto"
    ? `${formatMinute(group.schedule.startMinute)}-${formatMinute(
        group.schedule.endMinute
      )}`
    : "";
  // 时段说明（Zcode 的 19:00 开放 / 23:00 生效）优先于普通说明展示
  const note = meta.timing ?? [meta.note, window].filter(Boolean).join(" · ");

  // 免费模型核对信息，附在悬浮说明里（首页/封面共用）；积分/额度已在主文案
  const free = FREE_MODELS[group.provider.id];
  const freeSuffix = free
    ? free.models?.length
      ? ` · 免费模型：${free.models.join("、")}`
      : free.live
        ? " · 有免费模型（清单实时核对）"
        : ` · ${FREE_STATUS_TEXT[free.status]}`
    : "";

  if (meta.cadence === "oneTime") {
    return {
      label: "单账号一次性奖励",
      mode: meta.mode,
      note,
      title: `一次性福利：${meta.note ?? "注册奖励"} · 需手动领取（无每日签到）${freeSuffix}`,
    };
  }

  // **单账号**数量：各账号今日积分（Zcode 为每日 Token 额度），0 = 未加载/未开放。
  // 卡片主文案只说单账号拿多少；多账号合计放在悬浮说明里。
  const amounts = group.accounts.map((a) =>
    group.provider.id === "zcode" ? (a.quota?.totalGranted ?? 0) : (a.status?.points ?? 0)
  );
  const known = amounts.filter((v) => v > 0);

  if (known.length === 0) {
    return {
      label: "单账号每日福利",
      mode: meta.mode,
      note,
      title: `单账号每日福利数量未知${note ? ` · ${note}` : ""}${freeSuffix}`,
    };
  }

  const min = Math.min(...known);
  const max = Math.max(...known);
  const rangeText = min === max ? fmt(min) : `${fmt(min)}~${fmt(max)}`;
  const label = `单账号 +${rangeText} ${meta.unit}/日`;

  // 合计：只有多账号时才有意义；部分账号未加载时标注
  const total = known.reduce((s, v) => s + v, 0);
  const totalPart =
    amounts.length > 1
      ? ` · ${amounts.length} 个账号${
          known.length < amounts.length ? `（已加载 ${known.length}/${amounts.length}）` : ""
        }合计 ${fmt(total)} ${meta.unit}`
      : "";
  const title =
    meta.mode === "auto"
      ? `单账号每日 ${rangeText} ${meta.unit}${totalPart} · 自动领取${
          window ? `（时间窗 ${window}）` : ""
        }${meta.timing ? ` · ${meta.timing}` : ""}${freeSuffix}`
      : `单账号每日 ${rangeText} ${meta.unit}${totalPart} · 需手动领取${
          meta.note ? `（${meta.note}）` : ""
        }${meta.timing ? ` · ${meta.timing}` : ""}${freeSuffix}`;

  return { label, mode: meta.mode, note, title };
}
