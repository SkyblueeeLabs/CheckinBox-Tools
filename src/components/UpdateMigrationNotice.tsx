import { api } from "../lib/api";
import type { InstallInfo } from "../lib/types";
import { updateInstallKindLabel } from "../lib/update";

interface Props {
  info: InstallInfo;
  onClose: () => void;
}

export default function UpdateMigrationNotice({ info, onClose }: Props) {
  const old = info.oldInstalls[0];
  const portable = info.installKind === "portable";

  return (
    <div className="modal-overlay update-overlay" role="presentation">
      <div
        className="modal update-modal"
        role="dialog"
        aria-modal="true"
        aria-label="版本迁移"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="modal-header">
          <h2>版本迁移提示</h2>
          <button className="modal-close" onClick={onClose} aria-label="关闭">
            ×
          </button>
        </div>
        <div className="modal-body update-body">
          {old ? (
            <>
              <p className="update-hint">
                检测到旧版本 CheckinBox
                {old.version ? ` v${old.version}` : ""} 在：
              </p>
              <p className="update-path">{old.installLocation}</p>
              <p className="update-hint">
                建议先在 Windows「应用和功能」里卸载旧版本，避免两个版本并存。
                卸载不会删除账号和配置。
              </p>
            </>
          ) : (
            <p className="update-hint">
              当前是 {updateInstallKindLabel(info.installKind)}。自动安装只对
              NSIS 安装版开放；旧文件可以手动删除，账号和配置在
              %APPDATA%\CheckinBox 不会丢。
            </p>
          )}
          {portable && (
            <p className="update-hint">
              更新时会提示下载新版；如果你继续使用便携版，请用新版替换旧 exe。
            </p>
          )}
        </div>
        <div className="modal-footer">
          <button className="btn" onClick={onClose}>
            稍后
          </button>
          <button
            className="btn primary"
            onClick={() => {
              void api.openExternal("ms-settings:appsfeatures");
              onClose();
            }}
          >
            打开应用和功能
          </button>
        </div>
      </div>
    </div>
  );
}
