import { useCallback, useEffect, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { check, type DownloadEvent, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { api } from "../lib/api";
import type { AppConfig, InstallInfo, UpdateConfigPatch } from "../lib/types";
import {
  isTauriRuntime,
  isVersionNewer,
  portableDownloadUrl,
  releasePageUrl,
  shouldAutoCheck,
} from "../lib/update";
import type { AvailableUpdate, UpdateActionState } from "../components/UpdateDialog";

type NoticeKind = "ok" | "warn" | "err";

export interface VersionJumpInfo {
  version: string;
  notes: string;
}

export function useUpdater(
  config: AppConfig | null,
  setConfig: (config: AppConfig) => void,
  notify: (kind: NoticeKind, text: string) => void
) {
  const [currentVersion, setCurrentVersion] = useState("");
  const [installInfo, setInstallInfo] = useState<InstallInfo | null>(null);
  const [update, setUpdate] = useState<AvailableUpdate | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [checking, setChecking] = useState(false);
  const [action, setAction] = useState<UpdateActionState>("available");
  const [progress, setProgress] = useState(0);
  const [error, setError] = useState("");
  const [migrationNotice, setMigrationNotice] = useState<InstallInfo | null>(null);
  const [versionJump, setVersionJump] = useState<VersionJumpInfo | null>(null);

  const configRef = useRef(config);
  const notifyRef = useRef(notify);
  const setConfigRef = useRef(setConfig);
  const pluginUpdateRef = useRef<Update | null>(null);
  const checkingRef = useRef(false);
  const promptedRef = useRef<Set<string>>(new Set());
  const bootstrappedRef = useRef(false);
  const downloadedRef = useRef(0);
  const contentLengthRef = useRef(0);

  configRef.current = config;
  notifyRef.current = notify;
  setConfigRef.current = setConfig;

  const patchConfig = useCallback(async (patch: UpdateConfigPatch) => {
    const next = await api.patchUpdateConfig(patch);
    setConfigRef.current(next);
    return next;
  }, []);

  useEffect(() => {
    if (!config || bootstrappedRef.current) return;
    bootstrappedRef.current = true;
    if (!isTauriRuntime()) return;

    void (async () => {
      let version = "";
      try {
        version = await getVersion();
      } catch {
        // 非 Tauri 或读取失败时用后端版本兜底
      }

      let info: InstallInfo | null = null;
      try {
        info = await api.getInstallInfo();
        setInstallInfo(info);
      } catch {
        // 读不到安装信息时按便携版处理，仍可检查更新
      }
      if (!version && info?.version) {
        version = info.version;
      }
      setCurrentVersion(version);

      const updateConfig = configRef.current?.update;
      if (version) {
        const lastRun = updateConfig?.lastRunVersion ?? "";
        if (lastRun && lastRun !== version && isVersionNewer(version, lastRun)) {
          const notes =
            updateConfig?.pendingNotesVersion === version
              ? updateConfig?.pendingNotes ?? ""
              : "";
          setVersionJump({ version, notes });
        }
        try {
          await patchConfig({
            lastRunVersion: version,
            pendingNotesVersion: "",
            pendingNotes: "",
          });
        } catch {
          // 配置写失败不影响主流程
        }
      }

      if (info) {
        const migrationKey = `checkinbox.update.migration.${version || "unknown"}`;
        let dismissed = false;
        try {
          dismissed = localStorage.getItem(migrationKey) === "1";
        } catch {
          // localStorage 不可用时不记 dismissed
        }
        const hasOldInstall = info.oldInstalls.length > 0;
        if (!dismissed && (hasOldInstall || info.installKind !== "nsis")) {
          setMigrationNotice(info);
        }
      }
    })();
  }, [config, patchConfig]);

  const checkNow = useCallback(
    async (manual: boolean) => {
      if (checkingRef.current) return;
      if (!isTauriRuntime()) {
        if (manual) notifyRef.current("warn", "仅桌面版支持检查更新");
        return;
      }
      checkingRef.current = true;
      setChecking(true);
      setError("");
      try {
        const result = await check({ timeout: 30_000 });
        if (!result) {
          if (manual) notifyRef.current("ok", "已是最新版本");
          await patchConfig({ lastCheckAt: Math.floor(Date.now() / 1000) });
          return;
        }

        pluginUpdateRef.current = result;
        const dto: AvailableUpdate = {
          version: result.version,
          currentVersion: result.currentVersion,
          notes: result.body ?? "",
          date: result.date,
        };
        const skipped =
          configRef.current?.update?.skippedVersion === result.version;
        setUpdate(dto);
        setAction("available");
        setProgress(0);
        const shouldOpen =
          manual ||
          (!skipped && !promptedRef.current.has(result.version));
        if (shouldOpen) {
          promptedRef.current.add(result.version);
          setDialogOpen(true);
          if (!manual) {
            notifyRef.current("ok", `发现新版本 v${result.version}`);
          }
        } else if (skipped && !manual) {
          setUpdate(null);
        }
        await patchConfig({ lastCheckAt: Math.floor(Date.now() / 1000) });
      } catch (reason) {
        // 失败也算一次检查，避免自动检查在每次配置变化后立刻重试。
        try {
          await patchConfig({ lastCheckAt: Math.floor(Date.now() / 1000) });
        } catch {
          // 写配置失败不影响错误提示
        }
        if (manual) {
          notifyRef.current("err", `检查更新失败：${String(reason)}`);
        }
      } finally {
        checkingRef.current = false;
        setChecking(false);
      }
    },
    [patchConfig]
  );

  useEffect(() => {
    if (!config || !isTauriRuntime()) return;
    if (config.update?.autoCheck === false) return;
    if (!shouldAutoCheck(config.update?.lastCheckAt, config.update?.checkIntervalHours)) {
      return;
    }
    const timer = window.setTimeout(() => {
      void checkNow(false);
    }, 20_000);
    return () => window.clearTimeout(timer);
  }, [
    checkNow,
    config,
    config?.update?.autoCheck,
    config?.update?.checkIntervalHours,
    config?.update?.lastCheckAt,
  ]);

  const startUpdate = useCallback(async () => {
    const pluginUpdate = pluginUpdateRef.current;
    if (!pluginUpdate) return;
    if (installInfo?.installKind !== "nsis") {
      void api.openExternal(releasePageUrl());
      return;
    }
    setAction("downloading");
    setProgress(0);
    setError("");
    downloadedRef.current = 0;
    contentLengthRef.current = 0;
    try {
      await patchConfig({
        pendingNotesVersion: pluginUpdate.version,
        pendingNotes: pluginUpdate.body ?? "",
      });
      await pluginUpdate.downloadAndInstall((event: DownloadEvent) => {
        if (event.event === "Started") {
          downloadedRef.current = 0;
          contentLengthRef.current = event.data.contentLength ?? 0;
          setProgress(0);
        } else if (event.event === "Progress") {
          downloadedRef.current += event.data.chunkLength;
          const total = contentLengthRef.current;
          setProgress(
            total > 0
              ? Math.min(100, Math.round((downloadedRef.current / total) * 100))
              : Math.min(95, progress + 1)
          );
        } else {
          setProgress(100);
          setAction("installing");
        }
      });
      setAction("ready");
      setProgress(100);
      notifyRef.current("ok", "下载完成，重启后生效");
    } catch (reason) {
      setAction("error");
      setError(String(reason));
      notifyRef.current("err", `更新失败：${String(reason)}`);
    }
  }, [installInfo?.installKind, patchConfig, progress]);

  const restart = useCallback(async () => {
    try {
      await relaunch();
    } catch (reason) {
      notifyRef.current("err", `重启失败：${String(reason)}`);
    }
  }, []);

  const skipVersion = useCallback(async () => {
    const version = update?.version;
    if (!version) return;
    try {
      await patchConfig({ skippedVersion: version });
      setDialogOpen(false);
      setUpdate(null);
      notifyRef.current("ok", `已跳过 v${version}`);
    } catch (reason) {
      notifyRef.current("err", `保存跳过版本失败：${String(reason)}`);
    }
  }, [patchConfig, update?.version]);

  const clearSkippedVersion = useCallback(async () => {
    try {
      await patchConfig({ skippedVersion: "" });
      notifyRef.current("ok", "已取消跳过版本");
    } catch (reason) {
      notifyRef.current("err", `取消跳过失败：${String(reason)}`);
    }
  }, [patchConfig]);

  const manualDownload = useCallback(() => {
    const url = update
      ? installInfo?.installKind === "portable"
        ? portableDownloadUrl(update.version)
        : releasePageUrl()
      : releasePageUrl();
    void api.openExternal(url);
  }, [installInfo?.installKind, update]);

  const setAutoCheck = useCallback(
    async (enabled: boolean) => {
      try {
        await patchConfig({ autoCheck: enabled });
      } catch (reason) {
        notifyRef.current("err", `保存自动检查设置失败：${String(reason)}`);
      }
    },
    [patchConfig]
  );

  const openDialog = useCallback(() => {
    if (update) setDialogOpen(true);
  }, [update]);

  const closeDialog = useCallback(() => {
    if (action === "downloading" || action === "installing") return;
    setDialogOpen(false);
  }, [action]);

  const dismissMigration = useCallback(() => {
    if (migrationNotice) {
      try {
        localStorage.setItem(
          `checkinbox.update.migration.${currentVersion || "unknown"}`,
          "1"
        );
      } catch {
        // 忽略 localStorage 异常
      }
    }
    setMigrationNotice(null);
  }, [currentVersion, migrationNotice]);

  const dismissVersionJump = useCallback(() => {
    setVersionJump(null);
  }, []);

  return {
    currentVersion,
    installInfo,
    update,
    dialogOpen,
    checking,
    action,
    progress,
    error,
    migrationNotice,
    versionJump,
    checkNow,
    startUpdate,
    restart,
    skipVersion,
    clearSkippedVersion,
    manualDownload,
    setAutoCheck,
    openDialog,
    closeDialog,
    dismissMigration,
    dismissVersionJump,
    skippedVersion: config?.update?.skippedVersion ?? "",
    autoCheck: config?.update?.autoCheck !== false,
    lastCheckAt: config?.update?.lastCheckAt ?? 0,
  };
}
