import minimax from "../assets/agents/minimax.png";
import qoder from "../assets/agents/qoder.png";
import miaoshou from "../assets/agents/miaoshou.png";

/**
 * 各平台的官方图标（从本机客户端安装目录里取的原版）：
 *   MiniMax Code → 客户端 resources/icon.ico（取 256×256 帧）
 *   Qoder CN     → 客户端 application-icons/qoder-light.png
 *   妙手         → 客户端 resources/icon.png
 */
export const AGENT_ICONS: Record<string, string> = {
  minimax,
  qoder,
  miaoshou,
};

/** 取图标；没有的平台回退到 undefined，由调用方决定兜底显示。 */
export function agentIcon(providerId: string): string | undefined {
  return AGENT_ICONS[providerId];
}
