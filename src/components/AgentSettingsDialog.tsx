import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { api } from "../lib/api";
import type { ClientPathStatus, ProviderView } from "../lib/types";

interface Props {
  provider: ProviderView;
  children: ReactNode;
  prompt?: string;
  continueLabel?: string;
  onContinue?: () => void;
  onPathChanged: (path: string | null) => void;
  onClose: () => void;
}

const SOURCE_LABELS = {
  manual: "手动指定",
  running: "自动检测 · 运行中",
  installed: "自动检测 · 已找到",
};

export default function AgentSettingsDialog({
  provider, children, prompt, continueLabel, onContinue, onPathChanged, onClose,
}: Props) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  const [status, setStatus] = useState<ClientPathStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(true);
  const [scanned, setScanned] = useState(false);

  useEffect(() => {
    const dialog = dialogRef.current;
    if (dialog && !dialog.open) dialog.showModal();
    return () => { if (dialog?.open) dialog.close(); };
  }, []);

  useEffect(() => {
    let active = true;
    void api.getClientPath(provider.id).then((result) => {
      if (active) setStatus(result);
    }).catch((reason) => {
      if (active) setError(String(reason));
    }).finally(() => {
      if (active) setBusy(false);
    });
    return () => { active = false; };
  }, [provider.id]);

  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  };

  const savePath = async (path: string | null) => {
    const next = await api.setClientPath(provider.id, path);
    setStatus(next);
    setScanned(false);
    onPathChanged(next.path);
  };

  const path = status?.path ?? status?.configuredPath;
  const source = status?.source ? SOURCE_LABELS[status.source] : status?.configuredPath ? "手动指定 · 路径失效" : "自动检测";
  // 路径已正常检测到时，把高频的「自动化 / 换号」放前面；路径缺失需要引导定位时才置顶显示。
  const pathFirst = !busy && (!status?.path || !!prompt);
  const pathSection = (
    <section className="agent-settings-section">
      <div className="agent-settings-section-head">
        <h3>客户端路径</h3>
        <span>{source}</span>
      </div>
      <div className="agent-client-path" title={path ?? undefined}>{path ?? (busy ? "正在检测…" : "未找到客户端")}</div>
      <div className="agent-settings-actions">
        <button className="btn small" type="button" disabled={busy} onClick={() => void run(async () => {
          const next = await api.getClientPath(provider.id, true);
          setStatus(next);
          onPathChanged(next.path);
          setScanned(true);
        })}>重新检测</button>
        <button className="btn small" type="button" disabled={busy} onClick={() => void run(async () => {
          const selected = await api.pickClientExe();
          if (selected) await savePath(selected);
        })}>选择程序</button>
        {status?.configuredPath && (
          <button className="btn small" type="button" disabled={busy} onClick={() => void run(() => savePath(null))}>恢复自动</button>
        )}
      </div>
      {scanned && status?.configuredPath && (
        <p className="agent-settings-note">{status.detectedPath ? `自动检测：${status.detectedPath}` : "自动检测未找到其他路径"}</p>
      )}
      {(error || status?.error || (!status?.path && prompt)) && (
        <p className="agent-settings-error" role="alert">{error || status?.error || prompt}</p>
      )}
    </section>
  );

  return (
    <dialog ref={dialogRef} className="agent-settings-dialog" aria-labelledby={titleId} onClose={onClose}>
      <header className="agent-settings-head">
        <h2 id={titleId}>{provider.name} 设置</h2>
        <button className="agent-settings-close" type="button" aria-label="关闭设置" onClick={() => dialogRef.current?.close()}>×</button>
      </header>
      {pathFirst ? (
        <>
          {pathSection}
          {children}
        </>
      ) : (
        <>
          {children}
          {pathSection}
        </>
      )}
      {onContinue && (
        <div className="agent-settings-actions resume">
          <button className="btn small primary" type="button" disabled={busy || !status?.path} onClick={onContinue}>{continueLabel}</button>
        </div>
      )}
    </dialog>
  );
}
