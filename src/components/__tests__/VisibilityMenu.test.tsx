import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import VisibilityMenu from "../VisibilityMenu";
import type { ProviderView } from "../../lib/types";

const providers: ProviderView[] = [
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
  {
    id: "zcode",
    name: "Zcode",
    accent: "#111111",
    docsUrl: "",
    supportsDiscovery: false,
    supportsOAuth: false,
    configured: true,
    hasClient: true,
    supportsClaim: false,
  },
];

describe("VisibilityMenu（平台显示管理）", () => {
  it("每个平台两个开关，回调只带被改的字段", () => {
    const onChange = vi.fn();
    render(
      <VisibilityMenu
        providers={providers}
        visibility={{ zcode: { sidebar: false, home: true, launch: true } }}
        onChange={onChange}
        onClose={vi.fn()}
      />
    );
    const sidebarSwitch = screen.getByRole("checkbox", {
      name: "Zcode 在侧栏显示",
    }) as HTMLInputElement;
    expect(sidebarSwitch.checked).toBe(false);
    expect(
      (screen.getByRole("checkbox", { name: "Zcode 在首页显示" }) as HTMLInputElement).checked
    ).toBe(true);

    fireEvent.click(sidebarSwitch);
    expect(onChange).toHaveBeenCalledWith("zcode", { sidebar: true });
    fireEvent.click(screen.getByRole("checkbox", { name: "Qoder CN 在首页显示" }));
    expect(onChange).toHaveBeenCalledWith("qoder", { home: false });
    const launchSwitch = screen.getByRole("checkbox", {
      name: "Qoder CN 在启动页显示",
    }) as HTMLInputElement;
    expect(launchSwitch.checked).toBe(true);
    fireEvent.click(launchSwitch);
    expect(onChange).toHaveBeenCalledWith("qoder", { launch: false });
  });

  it("Esc 与点菜单外关闭；锚点按钮和菜单内部不触发关闭", () => {
    const onClose = vi.fn();
    render(
      <div>
        <button data-vis-toggle>锚点</button>
        <VisibilityMenu
          providers={providers}
          visibility={{}}
          onChange={vi.fn()}
          onClose={onClose}
        />
      </div>
    );
    fireEvent.pointerDown(
      screen.getByRole("checkbox", { name: "Qoder CN 在侧栏显示" })
    );
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.pointerDown(screen.getByText("锚点"));
    expect(onClose).not.toHaveBeenCalled();

    fireEvent.pointerDown(document.body);
    expect(onClose).toHaveBeenCalledOnce();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(2);
  });
});