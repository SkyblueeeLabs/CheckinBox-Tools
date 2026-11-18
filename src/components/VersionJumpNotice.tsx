import { ReleaseNotes } from "./UpdateDialog";

interface Props {
  version: string;
  notes: string;
  onClose: () => void;
}

export default function VersionJumpNotice({ version, notes, onClose }: Props) {
  return (
    <div className="modal-overlay update-overlay" role="presentation">
      <div
        className="modal update-modal"
        role="dialog"
        aria-modal="true"
        aria-label="已更新"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="modal-header">
          <h2>已更新到 v{version}</h2>
          <button className="modal-close" onClick={onClose} aria-label="关闭">
            ×
          </button>
        </div>
        <div className="modal-body update-body">
          <p className="update-hint">本次更新内容：</p>
          <section className="update-notes-block">
            <ReleaseNotes notes={notes} />
          </section>
        </div>
        <div className="modal-footer">
          <button className="btn primary" onClick={onClose}>
            知道了
          </button>
        </div>
      </div>
    </div>
  );
}
