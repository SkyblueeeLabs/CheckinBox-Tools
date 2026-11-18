import { describe, expect, it } from "vitest";
import {
  compareVersions,
  isVersionNewer,
  portableDownloadUrl,
  releasePageUrl,
  shouldAutoCheck,
  updateInstallKindLabel,
} from "../update";

describe("自动更新工具", () => {
  it("版本比较按数字段处理", () => {
    expect(compareVersions("0.10.0", "0.9.9")).toBe(1);
    expect(compareVersions("v0.2.0", "0.2.0")).toBe(0);
    expect(compareVersions("0.1.9", "0.2.0")).toBe(-1);
    expect(isVersionNewer("0.2.0", "0.1.0")).toBe(true);
    expect(isVersionNewer("0.1.0", "0.2.0")).toBe(false);
  });

  it("自动检查间隔默认 6 小时", () => {
    const now = 1_700_000_000_000;
    expect(shouldAutoCheck(0, 6, now)).toBe(true);
    expect(shouldAutoCheck(Math.floor(now / 1000) - 3600, 6, now)).toBe(false);
    expect(shouldAutoCheck(Math.floor(now / 1000) - 6 * 3600, 6, now)).toBe(true);
  });

  it("更新下载地址与安装形态文案", () => {
    expect(releasePageUrl()).toContain("SkyblueeeLabs/CheckinBox-Tools/releases/latest");
    expect(portableDownloadUrl("0.2.0")).toContain(
      "CheckinBox_0.2.0_portable.exe"
    );
    expect(updateInstallKindLabel("nsis")).toBe("安装版");
    expect(updateInstallKindLabel("msi")).toBe("MSI 版");
    expect(updateInstallKindLabel("portable")).toBe("便携版");
  });
});
