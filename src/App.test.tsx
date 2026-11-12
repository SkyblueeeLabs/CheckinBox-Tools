import { invoke } from "@tauri-apps/api/core";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import type { AppConfig, Notice, Overview } from "./lib/types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

const overview: Overview = {
  totalCount: 0, claimedCount: 0, todayPoints: 0, accounts: [], providers: [],
  schedulerEnabled: true, syncEnabled: true, agents: {}, plans: {}, dataDir: "",
};
const config: AppConfig = {
  scheduler: { enabled: true, syncEnabled: true, agents: {} },
  activeAccounts: {}, alertThresholds: {},
  closeAction: "ask", notificationsEnabled: true,
};
const notice: Notice = {
  id: "notice-1", kind: "warn", title: "额度即将到期", body: "请查看账户余额。",
  createdAt: 1_785_000_000_000, read: false,
};

describe("桌面窗口", () => {
  beforeEach(() => {
    vi.resetAllMocks();
  });

  it("首屏失败后可以重试，签到记录读取失败也能打开总览", async () => {
    let failOverview = true;
    vi.mocked(invoke).mockImplementation(async (command) => {
      switch (command) {
        case "overview":
          if (failOverview) throw new Error("临时加载失败");
          return overview;
        case "get_config": return config;
        case "get_logs": throw new Error("日志不可读");
        default: return [];
      }
    });

    render(<App />);
    expect(await screen.findByText("账号与积分加载失败")).toBeInTheDocument();
    failOverview = false;
    fireEvent.click(screen.getByRole("button", { name: "重试" }));
    expect(await screen.findByRole("button", { name: "一键全部签到" })).toBeEnabled();
  });

  it("应用未完成首屏加载时也能处理托盘提醒入口并标为已读", async () => {
    let read = false;
    vi.mocked(invoke).mockImplementation(async (command) => {
      switch (command) {
        case "overview": return new Promise(() => {});
        case "take_window_request": return "notices";
        case "list_notices": return [{ ...notice, read }];
        case "mark_notices_read": read = true; return;
        default: return [];
      }
    });

    render(<App />);
    const dialog = await screen.findByRole("dialog", { name: "应用内提醒" });
    expect(await within(dialog).findByText(notice.title)).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "全部标为已读" }));
    await waitFor(() => expect(within(dialog).queryByText("未读")).not.toBeInTheDocument());
    expect(within(dialog).getByRole("button", { name: "全部标为已读" })).toBeDisabled();
    expect(invoke).toHaveBeenCalledWith("mark_notices_read", { ids: [notice.id] });
  });
});
