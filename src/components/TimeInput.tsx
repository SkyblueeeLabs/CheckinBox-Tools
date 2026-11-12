import { useEffect, useState } from "react";
import { formatMinute, parseMinute } from "../lib/api";

/**
 * 时间窗输入（HH:MM）。
 *
 * ⚠️ 不能直接写成「`value={formatMinute(...)}` + onChange 里严格解析」的
 * 受控输入：React 在每次 change 后会把输入框恢复成上次渲染的 prop 值，
 * 而逐字符输入时中间态（"1"、"10:"）解析失败 → 状态不更新 → 立刻被打回，
 * 表现为「根本改不动」（2026-11-04 实测）。
 *
 * 这里的做法：本地存原始文本，解析成功才回写父组件；失焦时把文本规整成
 * 父组件的值（解析失败/被拒绝时会自然回弹）。
 */
export default function TimeInput({
  value,
  onCommit,
  className,
  ariaLabel,
}: {
  /** 当前分钟数（0-1439） */
  value: number;
  /** 解析成功时回写 */
  onCommit: (minute: number) => void;
  className?: string;
  ariaLabel?: string;
}) {
  const [text, setText] = useState(() => formatMinute(value));
  const [editing, setEditing] = useState(false);

  // 外部值变化时同步（编辑中不同步，否则会把用户正在输入的内容冲掉）
  useEffect(() => {
    if (!editing) setText(formatMinute(value));
  }, [value, editing]);

  return (
    <input
      className={className}
      aria-label={ariaLabel}
      value={text}
      inputMode="numeric"
      onFocus={() => setEditing(true)}
      onChange={(e) => {
        const next = e.target.value;
        setText(next);
        const m = parseMinute(next);
        if (m !== null) onCommit(m);
      }}
      onBlur={() => {
        setEditing(false);
        setText(formatMinute(value));
      }}
    />
  );
}
