import fs from "node:fs";
import path from "node:path";

function parseArgs(argv) {
  const args = {};
  for (let i = 0; i < argv.length; i += 1) {
    const token = argv[i];
    if (!token.startsWith("--")) continue;
    const key = token.slice(2);
    const value = argv[i + 1];
    if (!value || value.startsWith("--")) {
      args[key] = "true";
      continue;
    }
    args[key] = value;
    i += 1;
  }
  return args;
}

function required(args, key) {
  const value = args[key];
  if (!value) throw new Error(`missing --${key}`);
  return value;
}

const args = parseArgs(process.argv.slice(2));
const version = required(args, "version");
const repo = required(args, "repo");
const assetsDir = path.resolve(required(args, "assets-dir"));
const notesFile = path.resolve(required(args, "notes-file"));
const publishedAt = required(args, "published-at");
const output = path.resolve(args.output || "latest.json");

const files = fs.readdirSync(assetsDir);
const versionTag = version.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
const versionPattern = new RegExp(`_${versionTag}_`);
const nsisCandidates = files.filter(
  (name) =>
    versionPattern.test(name) &&
    /_x64-setup\.exe$/i.test(name) &&
    !/^uninstall/i.test(name)
);
const nsis =
  nsisCandidates[0] ||
  files.find(
    (name) => /_x64-setup\.exe$/i.test(name) && !/^uninstall/i.test(name)
  );
if (!nsis) {
  throw new Error(`no NSIS x64 setup installer found in ${assetsDir}`);
}

const readSignature = (asset) => {
  const sigName = `${asset}.sig`;
  if (!files.includes(sigName)) {
    throw new Error(`missing updater signature ${sigName}`);
  }
  const signature = fs.readFileSync(path.join(assetsDir, sigName), "utf8").trim();
  if (!signature) throw new Error(`empty updater signature ${sigName}`);
  return signature;
};

const assetUrl = (asset) =>
  `https://github.com/${repo}/releases/download/v${version}/${encodeURIComponent(asset)}`;

const platforms = {
  "windows-x86_64": {
    signature: readSignature(nsis),
    url: assetUrl(nsis),
  },
  "windows-x86_64-nsis": {
    signature: readSignature(nsis),
    url: assetUrl(nsis),
  },
};

const msi =
  files.find(
    (name) =>
      versionPattern.test(name) &&
      /\.msi$/i.test(name) &&
      !/^uninstall/i.test(name)
  ) ||
  files.find((name) => /\.msi$/i.test(name) && !/^uninstall/i.test(name));
if (msi && files.includes(`${msi}.sig`)) {
  platforms["windows-x86_64-msi"] = {
    signature: readSignature(msi),
    url: assetUrl(msi),
  };
}

const notes = fs.readFileSync(notesFile, "utf8").trim();
if (!notes) throw new Error(`release notes are empty: ${notesFile}`);

const timestamp = Date.parse(publishedAt);
if (Number.isNaN(timestamp)) {
  throw new Error(`invalid --published-at: ${publishedAt}`);
}

const manifest = {
  version,
  notes,
  pub_date: new Date(timestamp).toISOString(),
  platforms,
};

fs.mkdirSync(path.dirname(output), { recursive: true });
fs.writeFileSync(output, `${JSON.stringify(manifest, null, 2)}\n`, "utf8");
console.log(
  `updater manifest written to ${output} (${Object.keys(platforms).join(", ")})`
);
