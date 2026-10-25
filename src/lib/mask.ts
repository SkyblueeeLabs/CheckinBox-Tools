import { useEffect, useState } from "react";

/**
 * 账号 id 的打码显示（每个 agent 独立偏好，账号小卡片与签到记录共用）。
 *
 * id 是账号的唯一标识（名字会重名），作为副名字展示用来区分账号；
 * 打码后只显示 `头2****尾2`，区分度够、截图不泄露全号。
 */

const CHANGE_EVENT = "checkinbox-mask-change";

/** id 打码：留头 2 位尾 2 位，中间 ****。太短的（≤6 位）原样显示。 */
export function maskId(id: string): string {
  if (id.length <= 6) return id;
  return `${id.slice(0, 2)}****${id.slice(-2)}`;
}

function keyOf(providerId: string): string {
  return `checkinbox-log-mask:${providerId}`;
}

/** 某 agent 的打码开关（默认开）。 */
export function isMasked(providerId: string): boolean {
  return localStorage.getItem(keyOf(providerId)) !== "0";
}

/** 设置某 agent 的打码开关，并广播给同屏使用方（卡片/记录行即时刷新）。 */
export function setMasked(providerId: string, masked: boolean): void {
  localStorage.setItem(keyOf(providerId), masked ? "1" : "0");
  window.dispatchEvent(new CustomEvent(CHANGE_EVENT, { detail: providerId }));
}

/** 订阅式读取：开关变化或切换 agent 时自动刷新。 */
export function useMasked(providerId: string): boolean {
  const [masked, setLocal] = useState(() => isMasked(providerId));

  useEffect(() => {
    setLocal(isMasked(providerId));
    const onChange = (e: Event) => {
      if ((e as CustomEvent).detail === providerId) {
        setLocal(isMasked(providerId));
      }
    };
    window.addEventListener(CHANGE_EVENT, onChange);
    return () => window.removeEventListener(CHANGE_EVENT, onChange);
  }, [providerId]);

  return masked;
}
