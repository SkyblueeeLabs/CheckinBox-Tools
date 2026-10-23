import type { LogEntry } from "../lib/types";

interface Props {
  logs: LogEntry[];
  /** 当前选中的 agent id；`null` 表示「全部」 */
  providerId: string | null;
  /** 当前 agent 的展示名，用于标题 */
  providerName: string;
  /** provider id -> 展示名（「全部」视图里用） */
  providerNames: Record<string, string>;
  visible: boolean;
  onToggle: () => void;
}

const STATUS_TEXT: Record<string, string> = {
  success: "成功",
  already: "已签",
  failed: "失败",
  skipped: "跳过",
};

/**
 * 签到记录。
 *
 * 默认只显示**当前选中 agent** 的记录 —— 各平台的签到互不相关，
 * 混在一起看反而对不上号。切到「全部」才合并展示。
 */
export default function LogPanel({
  logs,
  providerId,
  providerName,
  providerNames,
  visible,
  onToggle,
}: Props) {
  const shown = providerId ? logs.filter((l) => l.provider === providerId) : logs;

  return (
    <section className="logs">
      <button className="logs-head" onClick={onToggle}>
        <h3>
          签到记录
          <span className="logs-count">
            {providerId ? providerName : "全部 agent"} · 近 7 天 · {shown.length} 条
          </span>
        </h3>
        <span className={`logs-caret ${visible ? "open" : ""}`}>▾</span>
      </button>

      {visible &&
        (shown.length === 0 ? (
          <p className="logs-empty">
            {providerId ? `${providerName} 最近 7 天没有签到记录。` : "还没有记录。"}
          </p>
        ) : (
          <div className={`logs-list ${providerId ? "single" : "all"}`}>
            {shown.map((log) => (
              <div key={log.id} className={`log-row ${log.status}`}>
                <span className="log-ts">{log.timestamp.slice(5)}</span>
                {!providerId && (
                  <span className="log-provider">
                    {providerNames[log.provider] ?? log.provider}
                  </span>
                )}
                <span className="log-label" title={log.label}>
                  {log.label}
                </span>
                <span className="log-what">{log.message}</span>
                <span className="log-tag">{STATUS_TEXT[log.status] ?? log.status}</span>
                {log.points > 0 ? (
                  <span className="log-pts">+{log.points}</span>
                ) : null}
              </div>
            ))}
          </div>
        ))}
    </section>
  );
}
