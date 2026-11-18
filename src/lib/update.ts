export const RELEASE_REPO = "SkyblueeeLabs/CheckinBox-Tools";

export function compareVersions(left: string, right: string): number {
  const parse = (value: string) =>
    value
      .trim()
      .replace(/^v/i, "")
      .split(/[^\d]+/)
      .filter(Boolean)
      .map((part) => Number.parseInt(part, 10))
      .filter((part) => Number.isFinite(part) && part >= 0);

  const a = parse(left);
  const b = parse(right);
  const length = Math.max(a.length, b.length);
  for (let i = 0; i < length; i += 1) {
    const x = a[i] ?? 0;
    const y = b[i] ?? 0;
    if (x > y) return 1;
    if (x < y) return -1;
  }
  return 0;
}

export function isVersionNewer(candidate: string, current: string): boolean {
  return compareVersions(candidate, current) > 0;
}

export function releasePageUrl(): string {
  return `https://github.com/${RELEASE_REPO}/releases/latest`;
}

export function portableDownloadUrl(version: string): string {
  return `https://github.com/${RELEASE_REPO}/releases/download/v${version}/CheckinBox_${version}_portable.exe`;
}

export function updateInstallKindLabel(kind: "nsis" | "msi" | "portable"): string {
  switch (kind) {
    case "nsis":
      return "安装版";
    case "msi":
      return "MSI 版";
    default:
      return "便携版";
  }
}

export function shouldAutoCheck(
  lastCheckAt: number | undefined,
  intervalHours: number | undefined,
  nowMs = Date.now()
): boolean {
  const interval = Math.max(1, intervalHours || 6);
  const last = (lastCheckAt || 0) * 1000;
  return nowMs - last >= interval * 3600 * 1000;
}

export function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}
