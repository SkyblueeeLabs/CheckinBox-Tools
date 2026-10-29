import type { LogEntry } from "../lib/types";
import { maskId, setMasked, useMasked } from "../lib/mask";

interface Props {
  logs: LogEntry[];
  /** 当前选中的 agent id；`null` 表示「全部」 */
  providerId: string | null;
  /** 当前 agent 的展示名，用于标题 */
  providerName: string;
  /** provider id -> 展示名（「全部」视图里用） */
  providerNames: Record<string, string>;
  /** `provider:accountId` -> 当前名字：按 id 解析记录归属账号的现名 */
  accountNames: Record<string, string>;
  visible: boolean;
  onToggle: () => void;
}

const STATUS_TEXT: Record<string, string> = {
  success: "成功",
  already: "已签",
  failed: "失败",
  skipped: "跳过",
};

/** 打码开关的存储键（每个 agent 独立记忆，与账号小卡片共用）。「全部」视图用 __all__。 */
function maskKeyOf(providerId: string | null): string {
  return providerId ?? "__all__";
}

/**
 * 签到记录。
 *
 * 默认只显示**当前选中 agent** 的记录 —— 各平台的签到互不相关，
 * 混在一起看反而对不上号。切到「全部」才合并展示。
 *
 * 账号名是**写入时的快照**，同名账号分不清 —— 所以每行在名字下面带一个
 * 副名字（账号 id，记录里永远有），顶部有每个 agent 独立的「打码」开关：
 * 打码时只显示 `头2****尾2`，够区分、不泄露。账号小卡片共用这个开关。
 */
export default function LogPanel({
  logs,
  providerId,
  providerName,
  providerNames,
  accountNames,
  visible,
  onToggle,
}: Props) {
  const masked = useMasked(maskKeyOf(providerId));

  const toggleMask = () => setMasked(maskKeyOf(providerId), !masked);

  const shown = providerId ? logs.filter((l) => l.provider === providerId) : logs;

  /**
   * 记录行的账号名：**按 id 解析当前名字**，解析不到（账号已移出池）才用
   * 写入时的快照。快照名字在账号改名/换人用旧名后会误导 —— 身份认 id。
   */
  const displayName = (log: LogEntry): { name: string; stale: boolean } => {
    const current = accountNames[`${log.provider}:${log.accountId}`];
    return current !== undefined ? { name: current, stale: false } : { name: log.label, stale: true };
  };

  return (
    <section className="logs">
      <div className="logs-head">
        <button className="logs-title" onClick={onToggle} aria-expanded={visible}>
          <h3>
            签到记录
            <span className="logs-count">
              {providerId ? providerName : "全部 agent"} · 近 7 天 · {shown.length} 条
            </span>
          </h3>
          <span className={`logs-caret ${visible ? "open" : ""}`}>▾</span>
        </button>
        <label className="switch" title="账号 id 打码显示（头2****尾2），关闭可看全号">
          <input type="checkbox" checked={masked} onChange={toggleMask} />
          <span>打码</span>
        </label>
      </div>

      {visible &&
        (shown.length === 0 ? (
          <p className="logs-empty">
            {providerId ? `${providerName} 最近 7 天没有签到记录。` : "还没有记录。"}
          </p>
        ) : (
          <div className={`logs-list ${providerId ? "single" : "all"}`}>
            {shown.map((log) => {
              const { name, stale } = displayName(log);
              return (
                <div key={log.id} className={`log-row ${log.status}`}>
                  <span className="log-ts">{log.timestamp.slice(5)}</span>
                  {!providerId && (
                    <span className="log-provider">
                      {providerNames[log.provider] ?? log.provider}
                    </span>
                  )}
                  <span className="log-label">
                    <span
                      className={`log-name${stale ? " stale" : ""}`}
                      title={stale ? `${name}（记录时的名字，该账号已不在账号池）` : name}
                    >
                      {name}
                    </span>
                    <span className="log-sub" title={log.accountId}>
                      {masked ? maskId(log.accountId) : log.accountId}
                    </span>
                  </span>
                  <span className="log-what">{log.message}</span>
                  <span className="log-tag">{STATUS_TEXT[log.status] ?? log.status}</span>
                  {log.points > 0 ? (
                    <span className="log-pts">+{log.points}</span>
                  ) : null}
                </div>
              );
            })}
          </div>
        ))}
    </section>
  );
}
