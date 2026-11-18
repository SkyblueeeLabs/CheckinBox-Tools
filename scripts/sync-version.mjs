import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const packagePath = path.join(root, "package.json");
const packageLockPath = path.join(root, "package-lock.json");
const tauriPath = path.join(root, "src-tauri", "tauri.conf.json");
const cargoPath = path.join(root, "src-tauri", "Cargo.toml");
const cargoLockPath = path.join(root, "src-tauri", "Cargo.lock");

const pkg = JSON.parse(fs.readFileSync(packagePath, "utf8"));
const version = String(pkg.version || "").trim();
if (!/^\d+\.\d+\.\d+(?:[-+].*)?$/.test(version)) {
  throw new Error(`package.json version is invalid: "${version}"`);
}

const writeIfChanged = (file, before, after) => {
  if (before === after) return false;
  fs.writeFileSync(file, after);
  return true;
};

const tauriBefore = fs.readFileSync(tauriPath, "utf8");
const tauriAfter = tauriBefore.replace(
  /"version"\s*:\s*"[^"]+"/,
  `"version": "${version}"`
);
const tauriChanged = writeIfChanged(tauriPath, tauriBefore, tauriAfter);

const cargoBefore = fs.readFileSync(cargoPath, "utf8");
const cargoAfter = cargoBefore.replace(
  /^version = "[^"]+"/m,
  `version = "${version}"`
);
const cargoChanged = writeIfChanged(cargoPath, cargoBefore, cargoAfter);

let lockChanged = false;
if (fs.existsSync(cargoLockPath)) {
  const lockBefore = fs.readFileSync(cargoLockPath, "utf8");
  const lockAfter = lockBefore.replace(
    /(\[\[package\]\]\r?\nname = "checkinbox"\r?\nversion = ")[^"]+(")/,
    `$1${version}$2`
  );
  lockChanged = writeIfChanged(cargoLockPath, lockBefore, lockAfter);
}

let packageLockChanged = false;
if (fs.existsSync(packageLockPath)) {
  const lock = JSON.parse(fs.readFileSync(packageLockPath, "utf8"));
  let changed = false;
  if (lock.version !== version) {
    lock.version = version;
    changed = true;
  }
  if (lock.packages?.[""] && lock.packages[""].version !== version) {
    lock.packages[""].version = version;
    changed = true;
  }
  if (changed) {
    fs.writeFileSync(packageLockPath, `${JSON.stringify(lock, null, 2)}\n`);
    packageLockChanged = true;
  }
}

console.log(
  `version ${version}: tauri=${tauriChanged ? "updated" : "ok"}, ` +
    `cargo=${cargoChanged ? "updated" : "ok"}, ` +
    `cargo-lock=${lockChanged ? "updated" : "ok"}, ` +
    `package-lock=${packageLockChanged ? "updated" : "ok"}`
);
