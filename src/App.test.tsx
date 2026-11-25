import { invoke } from "@tauri-apps/api/core";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { defaultAgentVisibility, type AppConfig, type Notice, type Overview } from "./lib/types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

const overview: Overview = {
  totalCount: 0, claimedCount: 0, todayPoints: 0, accounts: [], providers: [],
  schedulerEnabled: true, syncEnabled: true, agents: {}, plans: {}, dataDir: "",
};
const config: AppConfig = {
  scheduler: { enabled: true, syncEnabled: true, agents: {} },
  activeAccounts: {}, alertThresholds: {},
  closeAction: "ask", notificationsEnabled: true, uiRefreshSeconds: 45,
  agentVisibility: {},
};
const notice: Notice = {
  id: "notice-1", kind: "warn", title: "额度即将到期", body: "请查看账户余额。",
  createdAt: 1_785_000_000_000, read: false,
};

describe("桌面窗口", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    localStorage.clear();
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("离线")));
  });
  afterEach(() => vi.unstubAllGlobals());

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
    expect(await screen.findByText(/账号与积分加载失败/)).toBeInTheDocument();
    failOverview = false;
    fireEvent.click(screen.getByRole("button", { name: "重试" }));
    fireEvent.click(await screen.findByRole("button", { name: /进入 CheckinBox/ }));
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

  it("侧栏的官网和镜像站分别交给系统浏览器打开", async () => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "overview") return overview;
      if (command === "get_config") return config;
      return [];
    });

    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: /进入 CheckinBox/ }));
    const websites = within(await screen.findByRole("group", { name: "CheckinBox 网站" }));
    for (const [name, url] of [
      ["官网", "https://skyblueeelabs.github.io/CheckinBox-Tools/"],
      ["镜像站", "https://checkinbox-tools-uwpjuqh846z.qoder.zone/"],
    ]) {
      const link = websites.getByRole("link", { name });
      expect(link).toHaveAttribute("href", url);
      expect(link).toHaveAttribute("title", url);
      fireEvent.click(link);
      expect(invoke).toHaveBeenCalledWith("open_external", { url });
    }
  });

  it("首页公告离线可见，关闭后重启仍隐藏，新公告可正常显示", async () => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "overview") return overview;
      if (command === "get_config") return config;
      return [];
    });

    const openHome = async () => {
      const app = render(<App />);
      fireEvent.click(await screen.findByRole("button", { name: /进入 CheckinBox/ }));
      await screen.findByRole("button", { name: "一键全部签到" });
      return app;
    };

    const first = await openHome();
    expect(screen.getByRole("region", { name: "公告" })).toHaveTextContent(
      "ZCode 账号导入已修复 · 各 Agent 可在设置中自定义客户端路径",
    );
    fireEvent.click(screen.getByRole("button", { name: "关闭这条公告" }));
    expect(screen.queryByRole("region", { name: "公告" })).not.toBeInTheDocument();
    first.unmount();

    const second = await openHome();
    expect(screen.queryByRole("region", { name: "公告" })).not.toBeInTheDocument();
    second.unmount();

    const url = "https://skyblueeelabs.github.io/CheckinBox-Tools/";
    vi.mocked(fetch).mockResolvedValueOnce(new Response(JSON.stringify({
      id: "next-announcement", text: "有一条新公告", detailsUrl: url,
    })));
    const third = await openHome();
    const banner = await screen.findByRole("region", { name: "公告" });
    expect(banner).toHaveTextContent("有一条新公告");
    fireEvent.click(within(banner).getByRole("button", { name: "查看详情" }));
    expect(invoke).toHaveBeenCalledWith("open_external", { url });
    expect(banner).toBeInTheDocument();
    third.unmount();

    await openHome();
    expect(screen.getByRole("region", { name: "公告" })).toHaveTextContent("有一条新公告");
  });

  it("换号路径缺失时先选程序，登录态写入后的重试只启动客户端", async () => {
    const exe = "D:\\Custom\\WorkBuddy.exe";
    const ov: Overview = {
      ...overview,
      providers: [{
        id: "workbuddy", name: "WorkBuddy", accent: "#0052d9", docsUrl: "",
        supportsDiscovery: true, supportsOAuth: true, configured: true, hasClient: false, supportsClaim: true,
      }],
      agents: { workbuddy: {
        enabled: true, startMinute: 360, endMinute: 480, accountCount: 1,
        todayClaimed: 0, todayClaimable: 0, refreshIntervalMinutes: 10,
        switchable: true, chatSyncEnabled: false, chatSyncSupported: true,
      } },
      accounts: [{
        account: { id: "account-1", provider: "workbuddy", label: "测试账号", createdAt: 1, origin: "本机" },
        providerName: "WorkBuddy", accent: "#0052d9", isActive: false, isClientLogin: false,
        projectCount: 0, sessionCount: 0, isLow: false, alertThreshold: 200, trend: [], supportsClaim: true,
      }],
    };
    let selected = false;
    vi.mocked(invoke).mockImplementation(async (command) => {
      switch (command) {
        case "overview": return ov;
        case "get_config": return config;
        case "switch_client_account":
          if (!selected) throw { needsPath: true, message: "未检测到客户端" };
          return { targetFile: "auth.json", clientExe: exe, launchError: "等待启动超时" };
        case "get_client_path":
          return { path: null, configuredPath: null, source: null, detectedPath: null, error: "未检测到客户端" };
        case "pick_client_exe": return exe;
        case "set_client_path":
          selected = true;
          return { path: exe, configuredPath: exe, source: "manual", detectedPath: null, error: null };
        case "launch_client": return exe;
        default: return [];
      }
    });

    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: /进入 CheckinBox/ }));
    fireEvent.click(within(document.querySelector(".dash-agents")!).getByRole("button", { name: /WorkBuddy/ }));
    fireEvent.click(await screen.findByRole("button", { name: "切换并打开客户端" }));

    const settings = within(await screen.findByRole("dialog", { name: "WorkBuddy 设置" }));
    const continueSwitch = settings.getByRole("button", { name: "继续换号" });
    expect(continueSwitch).toBeDisabled();
    const pick = settings.getByRole("button", { name: "选择程序" });
    await waitFor(() => expect(pick).toBeEnabled());
    fireEvent.click(pick);
    await waitFor(() => expect(continueSwitch).toBeEnabled());
    expect(invoke).toHaveBeenCalledWith("set_client_path", { providerId: "workbuddy", path: exe });
    fireEvent.click(continueSwitch);

    const failed = within(await screen.findByRole("dialog", { name: "登录态已写入，启动失败" }));
    fireEvent.click(failed.getByRole("button", { name: "重试启动" }));
    expect(await screen.findByRole("dialog", { name: "客户端已启动" })).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("launch_client", { providerId: "workbuddy", previousExe: exe });
    expect(vi.mocked(invoke).mock.calls.filter(([command]) => command === "switch_client_account")).toHaveLength(2);
  });

  it("Qoder / ZCode 默认只在首页；从菜单打开侧栏、隐藏首页都会落盘", async () => {
    const ov: Overview = {
      ...overview,
      providers: [
        {
          id: "qoder", name: "Qoder CN", accent: "#6a5acd", docsUrl: "",
          supportsDiscovery: false, supportsOAuth: true, configured: true,
          hasClient: true, supportsClaim: true,
        },
        {
          id: "zcode", name: "ZCode", accent: "#111111", docsUrl: "",
          supportsDiscovery: false, supportsOAuth: false, configured: true,
          hasClient: true, supportsClaim: false,
        },
      ],
    };
    let cfg: AppConfig = { ...config, agentVisibility: {} };
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      switch (command) {
        case "overview": return ov;
        case "get_config": return cfg;
        case "set_agent_visibility": {
          const a = args as { providerId: string; sidebar: boolean | null; home: boolean | null; launch: boolean | null };
          const cur = cfg.agentVisibility[a.providerId] ?? defaultAgentVisibility(a.providerId);
          cfg = {
            ...cfg,
            agentVisibility: {
              ...cfg.agentVisibility,
              [a.providerId]: {
                sidebar: a.sidebar ?? cur.sidebar,
                home: a.home ?? cur.home,
                launch: a.launch ?? cur.launch,
              },
            },
          };
          return cfg;
        }
        case "autostart_enabled": return false;
        default: return [];
      }
    });

    render(<App />);
    // 启动页先展示主题标题，进入后才到主界面
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe(
      "今日的赛博鸡蛋，领了嘛？"
    );
    fireEvent.click(screen.getByRole("button", { name: /进入 CheckinBox/ }));
    expect(await screen.findByRole("button", { name: "一键全部签到" })).toBeEnabled();
    // 默认只上首页：首页两张卡，侧栏没有这两个平台
    expect(document.querySelectorAll(".dash-agent")).toHaveLength(2);
    expect(document.querySelector(".sidebar-list")?.textContent).not.toContain("Qoder CN");
    expect(document.querySelector(".sidebar-list")?.textContent).not.toContain("ZCode");

    fireEvent.click(screen.getByRole("button", { name: "管理平台显示" }));
    const menu = screen.getByRole("dialog", { name: "平台显示管理" });
    fireEvent.click(within(menu).getByRole("checkbox", { name: "Qoder CN 在侧栏显示" }));
    await waitFor(() =>
      expect(document.querySelector(".sidebar-list")?.textContent).toContain("Qoder CN")
    );
    // 打开侧栏时首页卡片不受影响
    expect(document.querySelectorAll(".dash-agent")).toHaveLength(2);

    fireEvent.click(within(menu).getByRole("checkbox", { name: "ZCode 在首页显示" }));
    await waitFor(() => expect(document.querySelectorAll(".dash-agent")).toHaveLength(1));
    expect(invoke).toHaveBeenCalledWith("set_agent_visibility", {
      providerId: "qoder", sidebar: true, home: null, launch: null,
    });
    expect(invoke).toHaveBeenCalledWith("set_agent_visibility", {
      providerId: "zcode", sidebar: null, home: false, launch: null,
    });
  });

  it("本机导入按实际落盘的账号提示新增或更新", async () => {
    const ov: Overview = {
      ...overview,
      providers: [
        {
          id: "workbuddy", name: "WorkBuddy", accent: "#6a5acd", docsUrl: "",
          supportsDiscovery: true, supportsOAuth: true, configured: false,
          hasClient: true, supportsClaim: true,
        },
      ],
    };
    const account = {
      id: "acc-1", provider: "workbuddy", label: "测试号",
      createdAt: 1, lastUsed: null, origin: "WorkBuddy 客户端登录态",
    };
    let alreadyAdded = false;
    vi.mocked(invoke).mockImplementation(async (command) => {
      switch (command) {
        case "overview": return ov;
        case "get_config": return config;
        case "autostart_enabled": return false;
        case "import_client_accounts": return [{ alreadyAdded, account }];
        default: return [];
      }
    });

    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: /进入 CheckinBox/ }));
    const dash = document.querySelector(".dash-agents") as HTMLElement;
    fireEvent.click(within(dash).getByRole("button", { name: /WorkBuddy/ }));
    fireEvent.click(await screen.findByRole("button", { name: "从本机导入" }));

    expect(await screen.findByText(/已导入 1 个账号：测试号/)).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("import_client_accounts", { providerId: "workbuddy" });
    alreadyAdded = true;
    fireEvent.click(screen.getByRole("button", { name: "从本机导入" }));
    expect(await screen.findByText("已更新账号登录态：测试号")).toBeInTheDocument();
  });

  it("导入没有真正写入时不谎报成功", async () => {
    const ov: Overview = {
      ...overview,
      providers: [
        {
          id: "qoder", name: "Qoder CN", accent: "#6a5acd", docsUrl: "",
          supportsDiscovery: true, supportsOAuth: true, configured: false,
          hasClient: true, supportsClaim: true,
        },
      ],
    };
    vi.mocked(invoke).mockImplementation(async (command) => {
      switch (command) {
        case "overview": return ov;
        case "get_config": return config;
        case "autostart_enabled": return false;
        case "import_client_accounts": throw new Error("写入账号池失败");
        default: return [];
      }
    });

    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: /进入 CheckinBox/ }));
    const dash = document.querySelector(".dash-agents") as HTMLElement;
    fireEvent.click(within(dash).getByRole("button", { name: /Qoder CN/ }));
    fireEvent.click(await screen.findByRole("button", { name: "从本机导入" }));

    expect(await screen.findByText(/导入失败.*写入账号池失败/)).toBeInTheDocument();
    expect(screen.queryByText(/已导入 1 个账号/)).not.toBeInTheDocument();
  });

  it("顶栏蛋图标随时打开封面浮层，Esc 返回原页面", async () => {
    const ov2: Overview = {
      ...overview,
      providers: [
        {
          id: "qoder", name: "Qoder CN", accent: "#6a5acd", docsUrl: "",
          supportsDiscovery: false, supportsOAuth: true, configured: true,
          hasClient: true, supportsClaim: true,
        },
      ],
    };
    vi.mocked(invoke).mockImplementation(async (command) => {
      switch (command) {
        case "overview": return ov2;
        case "get_config": return config;
        case "autostart_enabled": return false;
        default: return [];
      }
    });

    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: /进入 CheckinBox/ }));
    expect(await screen.findByRole("button", { name: "一键全部签到" })).toBeEnabled();

    fireEvent.click(screen.getByRole("button", { name: "看一眼赛博鸡蛋" }));
    const cover = screen.getByRole("dialog", { name: "赛博鸡蛋封面" });
    expect(within(cover).getByRole("heading", { level: 1 }).textContent).toBe(
      "今日的赛博鸡蛋，领了嘛？"
    );
    expect(within(cover).getByRole("button", { name: /返回原页面/ })).toBeInTheDocument();

    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() =>
      expect(screen.queryByRole("dialog", { name: "赛博鸡蛋封面" })).not.toBeInTheDocument()
    );
    expect(screen.getByRole("button", { name: "一键全部签到" })).toBeEnabled();
  });
});
