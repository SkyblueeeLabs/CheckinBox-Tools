import { useEffect, useRef } from "react";
import { agentVisibilityOf, type AgentVisibility, type ProviderView } from "../lib/types";
import { agentIcon, iconNeedsPad } from "../lib/agentIcons";

interface Props {
  providers: ProviderView[];
  visibility: Record<string, AgentVisibility>;
  onChange: (providerId: string, patch: Partial<AgentVisibility>) => void;
  onClose: () => void;
}

/**
 * 平台显示管理（侧栏 Agents 标题旁的眼睛按钮弹出）。
 * 每个平台三个开关：侧栏导航 / 首页卡片 / 启动页封面。隐藏只影响显示，
 * 自动签到与后台同步照常运行。Esc 或点菜单外关闭。
 */
export default function VisibilityMenu({ providers, visibility, onChange, onClose }: Props) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onDown = (e: PointerEvent) => {
      const target = e.target as Element;
      // 锚点按钮自己的 click 负责开合，这里不抢着关（否则关完又被 toggle 打开）
      if (target.closest("[data-vis-toggle]")) return;
      if (ref.current && !ref.current.contains(target)) onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("pointerdown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("pointerdown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [onClose]);

  return (
    <div className="vis-menu" role="dialog" aria-label="平台显示管理" ref={ref}>
      <div className="vis-menu-cols" aria-hidden="true">
        <span>平台</span>
        <span>侧栏</span>
        <span>首页</span>
        <span>启动页</span>
      </div>
      {providers.map((p) => {
        const vis = agentVisibilityOf(visibility, p.id);
        const icon = agentIcon(p.id);
        return (
          <div className="vis-row" key={p.id}>
            <span className="vis-name" title={p.name}>
              <span className="vis-mark">
                {icon ? (
                  <img
                    src={icon}
                    alt=""
                    className={`mark-img ${iconNeedsPad(p.id) ? "pad" : ""}`}
                    draggable={false}
                  />
                ) : (
                  p.name.slice(0, 1)
                )}
              </span>
              {p.name}
            </span>
            <input
              type="checkbox"
              checked={vis.sidebar}
              aria-label={`${p.name} 在侧栏显示`}
              onChange={(e) => onChange(p.id, { sidebar: e.target.checked })}
            />
            <input
              type="checkbox"
              checked={vis.home}
              aria-label={`${p.name} 在首页显示`}
              onChange={(e) => onChange(p.id, { home: e.target.checked })}
            />
            <input
              type="checkbox"
              checked={vis.launch}
              aria-label={`${p.name} 在启动页显示`}
              onChange={(e) => onChange(p.id, { launch: e.target.checked })}
            />
          </div>
        );
      })}
      <p className="vis-hint">隐藏只影响显示，自动签到与后台同步照常运行。</p>
    </div>
  );
}