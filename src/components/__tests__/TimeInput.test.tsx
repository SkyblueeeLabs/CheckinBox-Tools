import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import TimeInput from "../TimeInput";

/**
 * 回归测试（2026-11-04）：时间窗以前是「受控输入 + onChange 里严格解析」，
 * 逐字符输入时中间态解析失败 → 状态不更新 → React 把输入框恢复成旧值，
 * 表现为「根本改不动」。这里确保逐字符输入能正常提交。
 */
describe("TimeInput", () => {
  it("逐字符输入 10:30 能提交 630", () => {
    const onCommit = vi.fn();
    render(<TimeInput value={360} onCommit={onCommit} />);
    const input = screen.getByRole("textbox") as HTMLInputElement;

    fireEvent.focus(input);
    let text = "";
    for (const ch of "10:30") {
      text += ch;
      fireEvent.change(input, { target: { value: text } });
    }

    expect(input.value).toBe("10:30");
    expect(onCommit).toHaveBeenLastCalledWith(630);
  });

  it("输入 24:00 能提交 1440", () => {
    const onCommit = vi.fn();
    render(<TimeInput value={480} onCommit={onCommit} />);
    const input = screen.getByRole("textbox") as HTMLInputElement;

    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: "24:00" } });

    expect(onCommit).toHaveBeenLastCalledWith(1440);
  });

  it("非法文本不落库，失焦后回弹成父组件的值", () => {
    const onCommit = vi.fn();
    render(<TimeInput value={360} onCommit={onCommit} />);
    const input = screen.getByRole("textbox") as HTMLInputElement;

    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: "99:99" } });
    expect(onCommit).not.toHaveBeenCalled();

    fireEvent.blur(input);
    expect(input.value).toBe("06:00");
  });

  it("外部值变化（非编辑中）会同步到输入框", () => {
    const { rerender } = render(<TimeInput value={360} onCommit={vi.fn()} />);
    const input = screen.getByRole("textbox") as HTMLInputElement;
    expect(input.value).toBe("06:00");

    rerender(<TimeInput value={720} onCommit={vi.fn()} />);
    expect(input.value).toBe("12:00");
  });
});
