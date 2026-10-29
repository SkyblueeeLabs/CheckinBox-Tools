import { useCallback, useEffect, useState } from "react";
import { api, fmtBytes, fmtDateTime } from "../lib/api";
import type { ChatBackupEntry } from "../lib/types";

interface Props {
  /** provider id -> 显示名（来自 overview.providers） */
  providerNames: Record<string, string>;
  /** 每个 provider 的主题色，用于小圆点 */
  providerAccents: Record<string, string>;
}

/**
 * 换号聊天记录备份管理（设置页「历史备份」tab）。
 *
 * 数据源是后端 `list_chat_backups`：只列 `chat_history\<平台>\_backups\<操作id>\`
 * 这些**换号前的现场备份**。账号归档（`<账号id>\payload`）是同步功能本身的数据源，
 * 不在这里列出、也不允许删除 —— 删了对应账号的历史就回不来了。
 *
 * 删除是**不可撤销**的，所以每条都要用户确认；批量清理只处理时间能解析出来的
 * 旧备份（后端同样拒绝删时间未知的条目）。
 */
export default function BackupPanel({ providerNames, providerAccents }: Props) {
  const [entries, setEntries] = useState<ChatBackupEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [keepDays, setKeepDays] = useState(30);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setEntries(await api.listChatBackups());
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const removeOne = async (entry: ChatBackupEntry) => {
    const name = providerNames[entry.provider] ?? entry.provider;
    const when = fmtDateTime(entry.createdAtMs);
    if (!window.confirm(`删除 ${name} 在 ${when} 的备份（${fmtBytes(entry.bytes)}）？\n删除后无法恢复。`)) {
      return;
    }
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      setEntries(await api.deleteChatBackup(entry.provider, entry.opId));
      setNotice("已删除该备份");
    } catch (e) {
      setError(`删除失败：${e}`);
    } finally {
      setBusy(false);
    }
  };

  const removeOld = async () => {
    const days = Math.round(keepDays);
    if (!Number.isFinite(days) || days < 1) return;
    const candidates = entries.filter(
      (e) => e.createdAtMs > 0 && e.createdAtMs < Date.now() - days * 24 * 3600 * 1000
    );
    if (candidates.length === 0) {
      setNotice(`没有早于 ${days} 天的备份`);
      return;
    }
    const bytes = candidates.reduce((sum, e) => sum + e.bytes, 0);
    if (
      !window.confirm(
        `将删除 ${candidates.length} 份早于 ${days} 天的备份（合计 ${fmtBytes(bytes)}），删除后无法恢复。继续？`
      )
    ) {
      return;
    }
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      setEntries(await api.deleteOldChatBackups(days));
      setNotice(`已清理 ${candidates.length} 份旧备份`);
    } catch (e) {
      setError(`清理失败：${e}`);
    } finally {
      setBusy(false);
    }
  };

  const totalBytes = entries.reduce((sum, e) => sum + e.bytes, 0);

  return (
    <div className="set-block backup-panel">
      <div className="set-block-head">
        <h3>换号聊天记录备份</h3>
        <button className="btn small" onClick={() => void load()} disabled={loading || busy}>
          {loading ? "读取中…" : "刷新"}
        </button>
      </div>
      <p className="set-hint">
        每次「切换客户端账号」并开启聊天记录同步时，会先把现场聊天数据完整备份到这里
        （<code>chat_history\&lt;平台&gt;\_backups\&lt;操作id&gt;\</code>）。
        备份不会自动清理；确认换号无误后可以在这里删掉旧的。
        <b> 账号归档不在列表中</b>——它是同步功能的数据源，删除会导致该账号的历史丢失。
      </p>

      {error && <p className="backup-msg err">{error}</p>}
      {notice && <p className="backup-msg ok">{notice}</p>}

      <div className="backup-summary">
        <span>
          共 <b>{entries.length}</b> 份 · 合计 <b>{fmtBytes(totalBytes)}</b>
        </span>
        <span className="backup-cleanup">
          <span className="sched-hint">保留最近</span>
          <input
            className="backup-days"
            type="number"
            min={1}
            max={3650}
            value={keepDays}
            onChange={(e) => setKeepDays(Number(e.target.value))}
          />
          <span className="sched-hint">天，更早的</span>
          <button className="btn small danger" onClick={() => void removeOld()} disabled={busy || loading}>
            批量清理
          </button>
        </span>
      </div>

      {loading ? (
        <p className="set-hint">正在读取备份…</p>
      ) : entries.length === 0 ? (
        <div className="empty-state">
          <p>还没有换号备份。开启「换号同步聊天记录」并切换一次客户端账号后，这里会出现备份。</p>
        </div>
      ) : (
        <table className="set-table backup-table">
          <thead>
            <tr>
              <th>平台</th>
              <th>备份时间</th>
              <th className="r">大小</th>
              <th className="r">文件</th>
              <th>内容</th>
              <th className="r">操作</th>
            </tr>
          </thead>
          <tbody>
            {entries.map((e) => (
              <tr key={`${e.provider}:${e.opId}`}>
                <td>
                  <span
                    className="set-dot"
                    style={{ background: providerAccents[e.provider] ?? "#999" }}
                  />
                  {providerNames[e.provider] ?? e.provider}
                </td>
                <td className="backup-when">{fmtDateTime(e.createdAtMs)}</td>
                <td className="r num">{fmtBytes(e.bytes)}</td>
                <td className="r num">{e.files}</td>
                <td className="backup-items" title={e.items.join("\n")}>
                  {e.items.length > 0 ? e.items.join("、") : "—"}
                </td>
                <td className="r">
                  <button
                    className="btn small danger"
                    onClick={() => void removeOne(e)}
                    disabled={busy}
                  >
                    删除
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
