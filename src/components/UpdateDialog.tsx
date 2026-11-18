import type { ReactNode } from "react";
import { updateInstallKindLabel } from "../lib/update";

export interface AvailableUpdate {
  version: string;
  currentVersion: string;
  notes: string;
  date?: string;
}

export type UpdateActionState =
  | "available"
  | "downloading"
  | "installing"
  | "ready"
  | "error";

interface Props {
  open: boolean;
  update: AvailableUpdate | null;
  installKind: "nsis" | "msi" | "portable" | null;
  action: UpdateActionState;
  progress: number;
  error?: string;
  onClose: () => void;
  onUpdate: () => void;
  onSkip: () => void;
  onRestart: () => void;
  onManualDownload: () => void;
}

function renderInline(value: string): ReactNode[] {
  return value.split(/\*\*(.*?)\*\*/g).map((part, index) =>
    index % 2 === 1 ? <strong key={index}>{part}</strong> : part
  );
}

export function ReleaseNotes({ notes }: { notes: string }) {
  const text = notes.trim();
  if (!text) {
    return <p className="update-notes-empty">本次更新没有提供说明。</p>;
  }

  const lines = text.split(/\r?\n/);
  const elements: ReactNode[] = [];
  let list: ReactNode[] = [];
  const flushList = () => {
    if (list.length === 0) return;
    elements.push(<ul key={`list-${elements.length}`}>{list}</ul>);
    list = [];
  };

  for (const raw of lines) {
    const line = raw.trim();
    if (!line) {
      flushList();
      continue;
    }
    if (line.startsWith("### ")) {
      flushList();
      elements.push(<h4 key={`h4-${elements.length}`}>{line.slice(4)}</h4>);
    } else if (line.startsWith("## ")) {
      flushList();
      elements.push(<h3 key={`h3-${elements.length}`}>{line.slice(3)}</h3>);
    } else if (line.startsWith("- ")) {
      list.push(<li key={`li-${elements.length}-${list.length}`}>{renderInline(line.slice(2))}</li>);
    } else {
      flushList();
      elements.push(<p key={`p-${elements.length}`}>{renderInline(line)}</p>);
    }
  }
  flushList();

  return <div className="update-notes">{elements}</div>;
}

export default function UpdateDialog({
  open,
  update,
  installKind,
  action,
  progress,
  error,
  onClose,
  onUpdate,
  onSkip,
  onRestart,
  onManualDownload,
}: Props) {
  if (!open || !update) return null;

  const autoInstallable = installKind === "nsis";
  const installing = action === "downloading" || action === "installing";
  const ready = action === "ready";
  const failed = action === "error";
  const clamped = Math.max(0, Math.min(100, Math.round(progress)));

  return (
    <div className="modal-overlay update-overlay" role="presentation">
      <div
        className="modal update-modal"
        role="dialog"
        aria-modal="true"
        aria-label="软件更新"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="modal-header">
          <h2>软件更新</h2>
          <button
            className="modal-close"
            onClick={onClose}
            aria-label="关闭"
            disabled={installing}
          >
            ×
          </button>
        </div>

        <div className="modal-body update-body">
          <div className="update-version-line">
            <span className="update-version">v{update.version}</span>
            <span className="update-current">当前 v{update.currentVersion}</span>
            {update.date && <span className="update-date">{update.date}</span>}
          </div>

          {!autoInstallable && (
            <p className="update-hint">
              当前是 {updateInstallKindLabel(installKind ?? "portable")}，自动安装只对
              NSIS 安装版开放。可以下载新版手动替换；账号和配置不会丢。
            </p>
          )}

          {installing && (
            <div className="update-progress">
              <div className="update-progress-track">
                <div className="update-progress-fill" style={{ width: `${clamped}%` }} />
              </div>
              <span>
                {action === "installing" ? "正在安装…" : "正在下载…"} {clamped}%
              </span>
            </div>
          )}

          {ready && (
            <p className="update-hint update-ready">下载完成，重启后生效。</p>
          )}

          {failed && (
            <div className="update-error">
              <p>{error || "自动更新失败，可以手动下载新版。"}</p>
              <button className="btn small" onClick={onManualDownload}>
                手动下载
              </button>
            </div>
          )}

          <section className="update-notes-block">
            <h3>更新内容</h3>
            <ReleaseNotes notes={update.notes} />
          </section>
        </div>

        <div className="modal-footer update-actions">
          <button className="btn" onClick={onClose} disabled={installing}>
            稍后
          </button>
          {action === "available" && (
            <button className="btn" onClick={onSkip} disabled={installing}>
              跳过此版本
            </button>
          )}
          {ready ? (
            <button className="btn primary" onClick={onRestart}>
              重启
            </button>
          ) : autoInstallable ? (
            <button className="btn primary" onClick={onUpdate} disabled={installing}>
              {installing
                ? action === "installing"
                  ? "安装中…"
                  : "下载中…"
                : failed
                  ? "重试"
                  : "立即更新"}
            </button>
          ) : (
            <button className="btn primary" onClick={onManualDownload}>
              下载新版
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
