import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import LogPanel from "../LogPanel";
import type { LogEntry } from "../../lib/types";

function log(id: string, prov: string, message: string): LogEntry {
  return {
    id,
    timestamp: "2026-10-24 08:00:00",
    date: "2026-10-24",
    provider: prov,
    accountId: "abcd1234efgh",
    label: "账号",
    status: "success",
    points: 400,
    message,
  };
}

describe("LogPanel", () => {
  const logs = [log("1", "minimax", "+400 积分"), log("2", "qoder", "+100 Credits")];

  it("选中某个 agent 时只显示该平台的记录", () => {
    render(
      <LogPanel
        logs={logs}
        providerId="minimax"
        providerName="MiniMax Code"
        providerNames={{ minimax: "MiniMax Code", qoder: "Qoder CN" }}
        accountNames={{ "minimax:abcd1234efgh": "现名A" }}
        visible
        onToggle={() => {}}
      />
    );
    expect(screen.getByText("+400 积分")).toBeInTheDocument();
    expect(screen.queryByText("+100 Credits")).not.toBeInTheDocument();
  });

  it("「全部」视图合并展示并带平台列", () => {
    render(
      <LogPanel
        logs={logs}
        providerId={null}
        providerName="全部"
        providerNames={{ minimax: "MiniMax Code", qoder: "Qoder CN" }}
        accountNames={{ "minimax:abcd1234efgh": "现名A" }}
        visible
        onToggle={() => {}}
      />
    );
    expect(screen.getByText("+400 积分")).toBeInTheDocument();
    expect(screen.getByText("+100 Credits")).toBeInTheDocument();
    expect(screen.getByText("Qoder CN")).toBeInTheDocument();
  });

  it("收起时只保留标题和条数，不渲染记录行", () => {
    render(
      <LogPanel
        logs={logs}
        providerId={null}
        providerName="全部"
        providerNames={{ minimax: "MiniMax Code", qoder: "Qoder CN" }}
        accountNames={{}}
        visible={false}
        onToggle={() => {}}
      />
    );
    expect(screen.getByText(/近 7 天 · 2 条/)).toBeInTheDocument();
    expect(screen.queryByText("+400 积分")).not.toBeInTheDocument();
  });

  it("没有记录时显示空态文案", () => {
    render(
      <LogPanel
        logs={[]}
        providerId="minimax"
        providerName="MiniMax Code"
        providerNames={{}}
        accountNames={{}}
        visible
        onToggle={() => {}}
      />
    );
    expect(screen.getByText("MiniMax Code 最近 7 天没有签到记录。")).toBeInTheDocument();
  });

  it("每行带副名字（账号 id），默认打码为 头2****尾2", () => {
    localStorage.removeItem("checkinbox-log-mask:minimax");
    render(
      <LogPanel
        logs={[log("1", "minimax", "+400")]}
        providerId="minimax"
        providerName="MiniMax Code"
        providerNames={{}}
        accountNames={{}}
        visible
        onToggle={() => {}}
      />
    );
    expect(screen.getByText("ab****gh")).toBeInTheDocument();
  });

  it("关闭「打码」开关后显示完整 id，且偏好按 agent 记忆", () => {
    localStorage.removeItem("checkinbox-log-mask:minimax");
    render(
      <LogPanel
        logs={[log("1", "minimax", "+400")]}
        providerId="minimax"
        providerName="MiniMax Code"
        providerNames={{}}
        accountNames={{}}
        visible
        onToggle={() => {}}
      />
    );
    fireEvent.click(screen.getByText("打码"));
    expect(screen.getByText("abcd1234efgh")).toBeInTheDocument();
    expect(localStorage.getItem("checkinbox-log-mask:minimax")).toBe("0");
  });

  it("账号名按 id 解析当前名字；账号不在池里时退回快照名并淡化", () => {
    const renamed: LogEntry = { ...log("9", "minimax", "+100"), label: "旧名字", accountId: "zzzz9999xxxx" };
    render(
      <LogPanel
        logs={[log("1", "minimax", "+400"), renamed]}
        providerId="minimax"
        providerName="MiniMax Code"
        providerNames={{}}
        accountNames={{ "minimax:abcd1234efgh": "现名A" }}
        visible
        onToggle={() => {}}
      />
    );
    expect(screen.getByText("现名A")).toBeInTheDocument();
    expect(screen.queryByText("账号")).not.toBeInTheDocument();
    expect(screen.getByText("旧名字")).toBeInTheDocument();
    expect(document.querySelector(".log-name.stale")).toBeInTheDocument();
  });
});

