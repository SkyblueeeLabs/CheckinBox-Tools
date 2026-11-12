import { describe, expect, it } from "vitest";
import { formatMinute, parseMinute } from "../api";

describe("parseMinute / formatMinute", () => {
  it("常规时间", () => {
    expect(parseMinute("10:30")).toBe(630);
    expect(parseMinute("00:00")).toBe(0);
    expect(parseMinute("23:59")).toBe(1439);
    expect(parseMinute("6:00")).toBe(360);
  });

  it("分钟允许 1 位（手输友好）", () => {
    expect(parseMinute("10:3")).toBe(603);
  });

  it("允许 24:00（后端 is_valid 允许 end_minute <= 24*60）", () => {
    expect(parseMinute("24:00")).toBe(1440);
    expect(formatMinute(1440)).toBe("24:00");
  });

  it("非法输入返回 null", () => {
    expect(parseMinute("25:00")).toBeNull();
    expect(parseMinute("10:60")).toBeNull();
    expect(parseMinute("24:01")).toBeNull();
    expect(parseMinute("10")).toBeNull();
    expect(parseMinute("")).toBeNull();
    expect(parseMinute("abc")).toBeNull();
  });

  it("formatMinute 边界", () => {
    expect(formatMinute(0)).toBe("00:00");
    expect(formatMinute(1439)).toBe("23:59");
    expect(formatMinute(-5)).toBe("00:00");
  });
});
