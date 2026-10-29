import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import CloseConfirmDialog from "../CloseConfirmDialog";

describe("CloseConfirmDialog（关闭确认）", () => {
  it("默认不记住选择，点「最小化到托盘」返回 tray + false", () => {
    const onResolve = vi.fn();
    render(<CloseConfirmDialog onResolve={onResolve} onCancel={vi.fn()} />);

    const remember = screen.getByRole("checkbox") as HTMLInputElement;
    expect(remember.checked).toBe(false);

    fireEvent.click(screen.getByRole("button", { name: "最小化到托盘" }));
    expect(onResolve).toHaveBeenCalledWith("tray", false);
  });

  it("勾选「记住」后点退出，返回 exit + true", () => {
    const onResolve = vi.fn();
    render(<CloseConfirmDialog onResolve={onResolve} onCancel={vi.fn()} />);

    fireEvent.click(screen.getByRole("checkbox"));
    fireEvent.click(screen.getByRole("button", { name: "退出应用" }));
    expect(onResolve).toHaveBeenCalledWith("exit", true);
  });

  it("取消或按 Esc 只关闭弹窗，不执行动作", () => {
    const onResolve = vi.fn();
    const onCancel = vi.fn();
    render(<CloseConfirmDialog onResolve={onResolve} onCancel={onCancel} />);

    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    fireEvent.keyDown(window, { key: "Escape" });

    expect(onResolve).not.toHaveBeenCalled();
    expect(onCancel).toHaveBeenCalledTimes(2);
  });
});
