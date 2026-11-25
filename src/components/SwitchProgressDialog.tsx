import { useEffect, useRef } from "react";
import type { SwitchProgressStage } from "../lib/types";

interface Props {
  providerName: string;
  accountLabel: string;
  stage: SwitchProgressStage | "preparing" | "completed" | "failed" | "launchFailed";
  chatSyncEnabled: boolean;
  detail?: string;
  error?: string;
  onRetryLaunch: () => void;
  onEditPath: () => void;
  onClose: () => void;
}

const BASE_STEPS: Array<{ key: SwitchProgressStage; title: string; detail: string }> = [
  { key: "closing", title: "切换账号", detail: "正在关闭客户端" },
  { key: "capturing", title: "保存当前登录态", detail: "正在备份原账号信息" },
  { key: "validating", title: "检查目标账号", detail: "正在确认登录凭据有效" },
  { key: "syncing", title: "同步聊天记录", detail: "正在整理账号会话" },
  { key: "writing", title: "写入目标账号", detail: "正在更新客户端登录状态" },
  { key: "launching", title: "启动客户端", detail: "正在打开软件" },
];

export default function SwitchProgressDialog({
  providerName,
  accountLabel,
  stage,
  chatSyncEnabled,
  detail,
  error,
  onRetryLaunch,
  onEditPath,
  onClose,
}: Props) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const steps = BASE_STEPS.filter((step) => step.key !== "syncing" || chatSyncEnabled);
  const completed = stage === "completed";
  const launchFailed = stage === "launchFailed";
  const failed = stage === "failed" || launchFailed;
  const skippedSync = stage === "syncSkipped";
  const activeKey = skippedSync ? "writing" : stage;
  const activeIndex = steps.findIndex((step) => step.key === activeKey);

  useEffect(() => {
    const dialog = dialogRef.current;
    if (dialog && !dialog.open) dialog.showModal();
    return () => {
      if (dialog?.open) dialog.close();
    };
  }, []);

  return (
      <dialog
        ref={dialogRef}
        className="switch-progress-dialog"
        aria-labelledby="switch-progress-title"
        aria-describedby="switch-progress-subtitle"
        onCancel={(event) => {
          if (!completed && !failed) event.preventDefault();
        }}
        onClose={onClose}
      >
        <div className={`switch-progress-mark ${completed ? "success" : failed ? "failure" : ""}`} aria-hidden="true">
          {completed ? "✓" : failed ? "!" : <span />}
        </div>
        <h2 id="switch-progress-title">
          {completed ? "客户端已启动" : launchFailed ? "登录态已写入，启动失败" : failed ? "切换未完成" : "正在切换账号"}
        </h2>
        <p id="switch-progress-subtitle" className="switch-progress-subtitle">
          <strong>{providerName}</strong><span>·</span><span title={accountLabel}>{accountLabel}</span>
        </p>

        <ol className="switch-progress-steps">
          {steps.map((step, index) => {
            const isSkipped = skippedSync && step.key === "syncing";
            const isDone = completed || isSkipped || (launchFailed && step.key !== "launching")
              || (!failed && activeIndex >= 0 && index < activeIndex);
            const isCurrent = !completed && !failed && !isSkipped && index === activeIndex;
            return (
              <li key={step.key} className={`${isDone ? "done" : ""} ${isCurrent ? "current" : ""} ${isSkipped ? "skipped" : ""}`}>
                <span className="switch-step-mark" aria-hidden="true">{isDone ? "✓" : isCurrent ? <i /> : null}</span>
                <span className="switch-step-copy">
                  <strong>{isSkipped ? "无需同步记录" : step.title}</strong>
                  <small>{isSkipped ? "没有需要迁移的聊天记录" : step.detail}</small>
                </span>
              </li>
            );
          })}
        </ol>

        {(completed || launchFailed) && detail && <p className="switch-progress-result">{detail}</p>}
        {failed && <p className="switch-progress-error" role="alert">{error || "请检查客户端状态后重试。"}</p>}
        {(completed || failed) && (
          <div className="switch-progress-actions">
            {launchFailed && <button className="btn small" type="button" onClick={onEditPath}>调整路径</button>}
            <button className="btn small" type="button" onClick={() => dialogRef.current?.close()}>完成</button>
            {launchFailed && <button className="btn small primary" type="button" onClick={onRetryLaunch}>重试启动</button>}
          </div>
        )}
      </dialog>
  );
}
