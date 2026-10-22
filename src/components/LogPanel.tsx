import type { LogEntry } from "../lib/types";

interface Props {
  logs: LogEntry[];
}

const STATUS_TEXT: Record<string, string> = {
  success: "成功",
  already: "已签",
  failed: "失败",
  skipped: "跳过",
};

export default function LogPanel({ logs }: Props) {
  return (
    <section className="logs">
      <h3>签到记录<span className="count">近 7 天 · {logs.length} 条</span></h3>
      {logs.length === 0 ? (
        <p className="empty">还没有记录。</p>
      ) : (
        <div className="log-list">
          {logs.map((log) => (
            <div key={log.id} className={`log-row ${log.status}`}>
              <span className="ts">{log.timestamp.slice(5)}</span>
              <span className="who">{log.label}</span>
              <span className="what">{log.message}</span>
              <span className="tag">{STATUS_TEXT[log.status] ?? log.status}</span>
              {log.points > 0 ? <span className="pts">+{log.points}</span> : null}
            </div>
          ))}
        </div>
      )}
    </section>
  );
}
