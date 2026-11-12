import { useEffect, useRef, useState } from "react";

interface Props {
  onResolve: (action: "tray" | "exit", remember: boolean) => void;
  onCancel: () => void;
}

/**
 * 点窗口 × 时的确认框（关闭行为 = 每次询问）。
 *
 * 勾选「记住我的选择」后，选择会写进配置（config.closeAction），
 * 下次直接按这个行为处理，不再弹框。
 */
export default function CloseConfirmDialog({ onResolve, onCancel }: Props) {
  const [remember, setRemember] = useState(false);
  const dialog = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    const element = dialog.current;
    element?.showModal();
    return () => element?.close();
  }, []);

  const cancel = () => {
    dialog.current?.close();
    onCancel();
  };

  return (
    <dialog
      className="confirm-dialog"
      ref={dialog}
      aria-labelledby="close-dialog-title"
      onCancel={(e) => {
        e.preventDefault();
        cancel();
      }}
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) cancel();
      }}
    >
      <div className="modal-card">
        <h2 id="close-dialog-title">要关闭 CheckinBox 吗？</h2>
        <p className="modal-text">
          最小化到系统托盘后，自动签到和后台同步会在后台继续运行；
          选择退出则会彻底结束程序。
        </p>

        <label className="modal-check">
          <input
            type="checkbox"
            checked={remember}
            onChange={(e) => setRemember(e.target.checked)}
          />
          <span>记住我的选择，下次不再询问（可在设置里修改）</span>
        </label>

        <div className="modal-actions">
          <button className="btn" onClick={cancel}>
            取消
          </button>
          <button className="btn" onClick={() => onResolve("exit", remember)}>
            退出应用
          </button>
          <button
            className="btn primary"
            autoFocus
            onClick={() => onResolve("tray", remember)}
          >
            最小化到托盘
          </button>
        </div>
      </div>
    </dialog>
  );
}
