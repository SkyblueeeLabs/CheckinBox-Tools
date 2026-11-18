import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import UpdateDialog, { type AvailableUpdate } from "../UpdateDialog";

const update: AvailableUpdate = {
  version: "0.2.1",
  currentVersion: "0.2.0",
  date: "2026-11-18",
  notes: "### 新增\n- 测试更新\n- **重要** 修复",
};

function renderDialog(over: Partial<Parameters<typeof UpdateDialog>[0]> = {}) {
  return render(
    <UpdateDialog
      open
      update={update}
      installKind="nsis"
      action="available"
      progress={0}
      onClose={vi.fn()}
      onUpdate={vi.fn()}
      onSkip={vi.fn()}
      onRestart={vi.fn()}
      onManualDownload={vi.fn()}
      {...over}
    />
  );
}

describe("UpdateDialog", () => {
  it("安装版显示版本、更新内容和立即更新", () => {
    renderDialog();
    const dialog = screen.getByRole("dialog", { name: "软件更新" });
    expect(within(dialog).getByText("v0.2.1")).toBeInTheDocument();
    expect(within(dialog).getByText("当前 v0.2.0")).toBeInTheDocument();
    expect(within(dialog).getByText("测试更新")).toBeInTheDocument();
    expect(within(dialog).getByText("重要")).toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "立即更新" })).toBeEnabled();
    expect(within(dialog).getByRole("button", { name: "跳过此版本" })).toBeEnabled();
  });

  it("便携版不显示立即更新，改为下载新版", () => {
    renderDialog({ installKind: "portable" });
    expect(screen.getByRole("button", { name: "下载新版" })).toBeEnabled();
    expect(screen.queryByRole("button", { name: "立即更新" })).not.toBeInTheDocument();
    expect(screen.getByText(/自动安装只对/)).toBeInTheDocument();
  });

  it("下载中显示进度并禁止关闭", () => {
    renderDialog({ action: "downloading", progress: 42 });
    expect(screen.getByText(/正在下载… 42%/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "关闭" })).toBeDisabled();
  });

  it("下载完成后显示重启", () => {
    const onRestart = vi.fn();
    renderDialog({ action: "ready", progress: 100, onRestart });
    const button = screen.getByRole("button", { name: "重启" });
    fireEvent.click(button);
    expect(onRestart).toHaveBeenCalled();
  });
});
