import type { LogEntry } from "../lib/types";

interface Props {
  logs: LogEntry[];
  visible: boolean;
  onToggle: () => void;
}

const STATUS_TEXT: Record<string, string> = {
  success: "成功",
  already: "已签",
  failed: "失败",
  skipped: "跳过",
};

export default function LogPanel({ logs, visible, onToggle }: Props) {
  return (
    <section className="logs">
      <button className="logs-head" onClick={onToggle}>
        <h3>
          签到记录
          <span className="logs-count">近 7 天 · {logs.length} 条</span>
        </h3>
        <span className={`logs-caret ${visible ? "open" : ""}`}>▾</span>
      </button>

      {visible &&
        (logs.length === 0 ? (
          <p className="logs-empty">还没有记录。</p>
        ) : (
          <div className="logs-list">
            {logs.map((log) => (
              <div key={log.id} className={`log-row ${log.status}`}>
                <span className="log-ts">{log.timestamp.slice(5)}</span>
                <span className="log-who">{log.label}</span>
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
