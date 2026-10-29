import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import BackupPanel from "../BackupPanel";
import { api } from "../../lib/api";
import type { ChatBackupEntry } from "../../lib/types";

vi.mock("../../lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../lib/api")>();
  return {
    ...actual,
    api: {
      listChatBackups: vi.fn(),
      deleteChatBackup: vi.fn(),
      deleteOldChatBackups: vi.fn(),
    },
  };
});

function entry(over: Partial<ChatBackupEntry> = {}): ChatBackupEntry {
  return {
    provider: "qoder",
    opId: "1700000000000-1234",
    createdAtMs: 1700000000000,
    files: 3,
    bytes: 1024 * 1024,
    items: ["main.sqlite", "chat-session-turn-payload-buffer.sqlite"],
    ...over,
  };
}

const names = { qoder: "Qoder CN", workbuddy: "WorkBuddy" };
const accents = { qoder: "#6a5acd", workbuddy: "#0052d9" };

function renderPanel() {
  return render(<BackupPanel providerNames={names} providerAccents={accents} />);
}

beforeEach(() => {
  vi.mocked(api.listChatBackups).mockResolvedValue([]);
  vi.mocked(api.deleteChatBackup).mockResolvedValue([]);
  vi.mocked(api.deleteOldChatBackups).mockResolvedValue([]);
  vi.spyOn(window, "confirm").mockReturnValue(true);
});

afterEach(() => {
  vi.clearAllMocks();
  vi.restoreAllMocks();
});

describe("BackupPanel（换号备份管理）", () => {
  it("按平台展示备份的时间、大小、文件数和内容", async () => {
    vi.mocked(api.listChatBackups).mockResolvedValue([
      entry(),
      entry({
        provider: "workbuddy",
        opId: "1800000000000-9",
        createdAtMs: 1800000000000,
        files: 12,
        bytes: 2048,
        items: ["history"],
      }),
    ]);
    renderPanel();

    expect(await screen.findByText("Qoder CN")).toBeInTheDocument();
    expect(screen.getByText("WorkBuddy")).toBeInTheDocument();
    // 行内大小（合计行也可能显示同样的数字，用 td 限定）
    expect(screen.getByText("1.0 MB", { selector: "td" })).toBeInTheDocument();
    expect(screen.getByText("2.0 KB", { selector: "td" })).toBeInTheDocument();
    expect(screen.getByText("12", { selector: "td" })).toBeInTheDocument();
    expect(screen.getByText(/main\.sqlite/)).toBeInTheDocument();
    expect(screen.getByText("history")).toBeInTheDocument();
    expect(screen.getByText(/共/)).toHaveTextContent("共 2 份");
  });

  it("没有备份时显示空态", async () => {
    renderPanel();
    expect(
      await screen.findByText(/还没有换号备份/)
    ).toBeInTheDocument();
  });

  it("删除单条：确认后调用接口并刷新列表", async () => {
    vi.mocked(api.listChatBackups).mockResolvedValue([entry()]);
    vi.mocked(api.deleteChatBackup).mockResolvedValue([]);
    renderPanel();

    fireEvent.click(await screen.findByRole("button", { name: "删除" }));
    await waitFor(() =>
      expect(api.deleteChatBackup).toHaveBeenCalledWith("qoder", "1700000000000-1234")
    );
    expect(await screen.findByText(/还没有换号备份/)).toBeInTheDocument();
  });

  it("取消确认时不删除", async () => {
    vi.mocked(api.listChatBackups).mockResolvedValue([entry()]);
    vi.mocked(window.confirm).mockReturnValue(false);
    renderPanel();

    fireEvent.click(await screen.findByRole("button", { name: "删除" }));
    expect(api.deleteChatBackup).not.toHaveBeenCalled();
  });

  it("批量清理：只统计早于保留天数的备份并调用接口", async () => {
    const now = Date.now();
    vi.mocked(api.listChatBackups).mockResolvedValue([
      entry({ opId: `${now - 40 * 86400_000}-1`, createdAtMs: now - 40 * 86400_000 }),
      entry({ opId: `${now - 86400_000}-2`, createdAtMs: now - 86400_000 }),
    ]);
    renderPanel();

    fireEvent.click(await screen.findByRole("button", { name: "批量清理" }));
    await waitFor(() => expect(api.deleteOldChatBackups).toHaveBeenCalledWith(30));
    expect(screen.getByText(/已清理 1 份旧备份/)).toBeInTheDocument();
  });
});
