import { useEffect, useRef } from "react";
import { fmtDateTime } from "../lib/api";
import type { Notice } from "../lib/types";

interface Props {
  notices: Notice[] | null;
  error: string | null;
  marking: boolean;
  onMarkRead: () => void;
  onRetry: () => void;
  onClose: () => void;
}

export default function NotificationsDialog({
  notices, error, marking, onMarkRead, onRetry, onClose,
}: Props) {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const element = dialog.current;
    element?.showModal();
    return () => element?.close();
  }, []);
  const unread = notices?.filter((notice) => !notice.read).length ?? 0;

  return (
    <dialog className="notices-dialog" ref={dialog} aria-labelledby="notices-title"
      onCancel={(event) => {
        event.preventDefault();
        onClose();
      }}
    >
      <header className="notices-header">
        <div>
          <h2 id="notices-title">应用内提醒</h2>
          <p>{unread > 0 ? `${unread} 条未读 · ` : ""}保留最近 100 条</p>
        </div>
        <div className="notices-actions">
          <button className="btn small" onClick={onMarkRead} disabled={unread === 0 || marking}>
            {marking ? "正在标记…" : "全部标为已读"}
          </button>
          <button className="btn small" onClick={onClose} autoFocus>关闭</button>
        </div>
      </header>
      {error && (
        <div className="notices-empty" role="alert">
          <p>{error}</p>
          <button className="btn small" onClick={onRetry}>重试</button>
        </div>
      )}
      {!error && notices === null && <p className="notices-empty" role="status">正在读取提醒…</p>}
      {!error && notices?.length === 0 && <p className="notices-empty">暂无提醒</p>}
      {!!notices?.length && (
        <ul className="notices-list">
          {notices.map((notice) => (
            <li key={notice.id} className={`notice-row ${notice.kind}`}>
              <div className="notice-meta">
                <strong>{notice.title}</strong>
                {!notice.read && <span className="notice-unread">未读</span>}
                <time dateTime={new Date(notice.createdAt).toISOString()}>{fmtDateTime(notice.createdAt)}</time>
              </div>
              <p>{notice.body}</p>
            </li>
          ))}
        </ul>
      )}
    </dialog>
  );
}
