import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import SettingsPanel from "../SettingsPanel";
import { api } from "../../lib/api";
import type { AppConfig, Overview } from "../../lib/types";

vi.mock("../../lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../lib/api")>();
  return {
    ...actual,
    api: {
      listChatBackups: vi.fn().mockResolvedValue([]),
      deleteChatBackup: vi.fn(),
      deleteOldChatBackups: vi.fn(),
    },
  };
});

function overview(): Overview {
  return {
    accounts: [],
    providers: [
      {
        id: "qoder",
        name: "Qoder CN",
        accent: "#6a5acd",
        docsUrl: "",
        supportsDiscovery: false,
        supportsOAuth: true,
        configured: true,
        hasClient: true,
        supportsClaim: true,
      },
    ],
    todayPoints: 0,
    claimedCount: 0,
    totalCount: 0,
    schedulerEnabled: true,
    syncEnabled: true,
    agents: {},
    plans: {},
    dataDir: "C:\\Users\\test\\AppData\\Roaming\\CheckinBox",
  };
}

function config(): AppConfig {
  return {
    scheduler: { enabled: true, syncEnabled: true, agents: {} },
    activeAccounts: {},
    alertThresholds: {},
    closeAction: "ask",
    notificationsEnabled: true,
    uiRefreshSeconds: 45,
    agentVisibility: {},
  };
}

const noop = vi.fn();

function renderSettings(
  onCloseAction: (action: "ask" | "tray" | "exit") => void = noop,
  onNotificationsEnabled: (enabled: boolean) => void = noop,
  onUiRefreshInterval: (seconds: number) => void = noop
) {
  return render(
    <SettingsPanel
      overview={overview()}
      config={config()}
      busy={false}
      syncing={false}
      onSchedulerEnabled={noop}
      onSyncEnabled={noop}
      onRefreshInterval={noop}
      onUiRefreshInterval={onUiRefreshInterval}
      onScheduleChange={noop}
      onSyncNow={noop}
      onOpenDataDir={noop}
      onCloseAction={onCloseAction}
      autostartEnabled={false}
      onAutostartChange={noop}
      onNotificationsEnabled={onNotificationsEnabled}
      onTestNotification={noop}
    />
  );
}

describe("SettingsPanel（设置页 tab）", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("默认在「常规」tab，显示自动同步设置", () => {
    renderSettings();
    expect(screen.getByRole("tab", { name: "常规" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByText("自动同步")).toBeInTheDocument();
  });

  it("切到「历史备份」tab 后展示备份管理，且不再显示常规设置", async () => {
    renderSettings();
    fireEvent.click(screen.getByRole("tab", { name: "历史备份" }));

    expect(screen.getByText("换号聊天记录备份")).toBeInTheDocument();
    expect(screen.queryByText("自动签到调度")).not.toBeInTheDocument();
    expect(api.listChatBackups).toHaveBeenCalled();
  });

  it("设置卡片默认收起，点标题展开", () => {
    renderSettings();
    const toggle = screen.getByRole("button", { name: /自动签到调度/ });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(
      screen.queryByText(/总闸关掉后所有平台都不再自动签到/)
    ).not.toBeInTheDocument();

    fireEvent.click(toggle);

    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(
      screen.getByText(/总闸关掉后所有平台都不再自动签到/)
    ).toBeInTheDocument();
  });

  it("「关闭窗口」三选一：选中最小化到托盘时回调 tray", () => {
    const onCloseAction = vi.fn();
    renderSettings(onCloseAction);

    fireEvent.click(screen.getByRole("button", { name: /关闭窗口/ }));

    const ask = screen.getByRole("radio", { name: /每次询问/ }) as HTMLInputElement;
    expect(ask.checked).toBe(true);

    fireEvent.click(screen.getByRole("radio", { name: /最小化到系统托盘/ }));
    expect(onCloseAction).toHaveBeenCalledWith("tray");
  });

  it("应用内提醒开关回调当前值", () => {
    const onNotificationsEnabled = vi.fn();
    renderSettings(noop, onNotificationsEnabled);

    const block = screen.getByRole("button", { name: /应用内提醒/ }).closest(".set-block");
    expect(block).not.toBeNull();
    const toggle = within(block as HTMLElement).getByRole("checkbox") as HTMLInputElement;
    expect(toggle.checked).toBe(true);

    fireEvent.click(toggle);
    expect(onNotificationsEnabled).toHaveBeenCalledWith(false);
  });

  it("界面自动刷新间隔：预设直接生效，可关闭，自定义秒数钳制到 10–3600", () => {
    const onUiRefreshInterval = vi.fn();
    renderSettings(noop, noop, onUiRefreshInterval);
    fireEvent.click(screen.getByRole("button", { name: /自动同步/ }));

    const select = screen.getByRole("combobox", { name: "界面自动刷新间隔" });
    expect(select).toHaveValue("45");

    fireEvent.change(select, { target: { value: "60" } });
    expect(onUiRefreshInterval).toHaveBeenCalledWith(60);

    fireEvent.change(select, { target: { value: "0" } });
    expect(onUiRefreshInterval).toHaveBeenCalledWith(0);

    fireEvent.change(select, { target: { value: "custom" } });
    const input = screen.getByRole("spinbutton", { name: "自定义界面刷新秒数" });
    fireEvent.change(input, { target: { value: "9999" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onUiRefreshInterval).toHaveBeenCalledWith(3600);
    expect(
      screen.queryByRole("spinbutton", { name: "自定义界面刷新秒数" })
    ).not.toBeInTheDocument();
  });
});
