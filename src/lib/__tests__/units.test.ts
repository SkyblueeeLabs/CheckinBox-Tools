import { describe, expect, it } from "vitest";
import { fmtCompact, fmtPoints, fmtQuotaAmount, quotaLabels, quotaUnit } from "../api";

describe("quotaUnit / quotaLabels（Zcode 额度是 Token，其它平台是积分）", () => {
  it("zcode 判定为 Token，标签不带「总」字", () => {
    expect(quotaUnit("zcode")).toBe("Token");
    expect(quotaLabels("zcode")).toEqual({
      unit: "Token",
      remaining: "剩余 Token",
      granted: "累计发放 Token",
    });
  });

  it("其它平台与缺省保持积分文案不变", () => {
    for (const id of ["minimax", "qoder", "workbuddy", "miaoshou", undefined, null, ""]) {
      expect(quotaUnit(id)).toBe("积分");
      expect(quotaLabels(id)).toEqual({
        unit: "积分",
        remaining: "剩余总积分",
        granted: "累计发放总积分",
      });
    }
  });
});

describe("fmtCompact（大数缩写，末尾 0 舍去）", () => {
  it("亿级：3 亿显示成 3亿，带零头保留两位", () => {
    expect(fmtCompact(300_000_000)).toBe("3亿");
    expect(fmtCompact(298_890_433)).toBe("2.99亿");
    expect(fmtCompact(120_000_000)).toBe("1.2亿");
  });

  it("万级保留一位小数（四舍五入），万以下维持千分位", () => {
    expect(fmtCompact(1_109_567)).toBe("111万");
    expect(fmtCompact(1_234_567)).toBe("123.5万");
    expect(fmtCompact(12_345)).toBe("1.2万");
    expect(fmtCompact(999)).toBe("999");
    expect(fmtCompact(1234)).toBe("1,234");
  });

  it("边界与非法值", () => {
    expect(fmtCompact(0)).toBe("0");
    expect(fmtCompact(10_000)).toBe("1万");
    expect(fmtCompact(100_000_000)).toBe("1亿");
    expect(fmtCompact(9999)).toBe("9,999");
    expect(fmtCompact(NaN)).toBe("0");
    expect(fmtCompact(Infinity)).toBe("0");
  });
});

describe("fmtQuotaAmount（按平台选格式）", () => {
  it("Token 平台缩写，积分平台千分位", () => {
    expect(fmtQuotaAmount(298_890_433, "zcode")).toBe("2.99亿");
    expect(fmtQuotaAmount(298_890_433, "minimax")).toBe("298,890,433");
    expect(fmtQuotaAmount(400, "zcode")).toBe("400");
    expect(fmtQuotaAmount(400)).toBe("400");
  });

  it("与 fmtPoints 在积分平台上完全一致", () => {
    for (const n of [0, 399.6, 12_345, 987_654_321]) {
      expect(fmtQuotaAmount(n, "qoder")).toBe(fmtPoints(n));
    }
  });
});
