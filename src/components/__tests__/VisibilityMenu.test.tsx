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
    name: "ZCode",
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
  it("Qoder / ZCode 默认只上首页；显式配置可覆盖，回调只带被改的字段", () => {
    const onChange = vi.fn();
    render(
      <VisibilityMenu
        providers={providers}
        visibility={{ zcode: { sidebar: false, home: true, launch: true } }}
        onChange={onChange}
        onClose={vi.fn()}
      />
    );
    // Qoder 没配置过 → 按平台默认：侧栏 / 启动页关闭，首页打开
    expect(
      (screen.getByRole("checkbox", { name: "Qoder CN 在侧栏显示" }) as HTMLInputElement).checked
    ).toBe(false);
    expect(
      (screen.getByRole("checkbox", { name: "Qoder CN 在启动页显示" }) as HTMLInputElement).checked
    ).toBe(false);
    // ZCode 显式配置了 launch=true，覆盖平台默认
    const zcodeSidebar = screen.getByRole("checkbox", {
      name: "ZCode 在侧栏显示",
    }) as HTMLInputElement;
    expect(zcodeSidebar.checked).toBe(false);
    expect(
      (screen.getByRole("checkbox", { name: "ZCode 在启动页显示" }) as HTMLInputElement).checked
    ).toBe(true);

    fireEvent.click(zcodeSidebar);
    expect(onChange).toHaveBeenCalledWith("zcode", { sidebar: true });
    fireEvent.click(screen.getByRole("checkbox", { name: "Qoder CN 在首页显示" }));
    expect(onChange).toHaveBeenCalledWith("qoder", { home: false });
    fireEvent.click(screen.getByRole("checkbox", { name: "Qoder CN 在启动页显示" }));
    expect(onChange).toHaveBeenCalledWith("qoder", { launch: true });
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