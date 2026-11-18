import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

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

const args = parseArgs(process.argv.slice(2));
const packageVersion = JSON.parse(
  fs.readFileSync(path.join(root, "package.json"), "utf8")
).version;
const version = String(args.version || packageVersion).trim();
const changelogPath = path.resolve(
  root,
  args.file || "CHANGELOG.md"
);
const outputPath = args.output ? path.resolve(root, args.output) : null;

const escapeRegex = (value) => value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
const header = new RegExp(`^## \\[v?${escapeRegex(version)}\\](?:\\s*-.*)?$`);
const lines = fs.readFileSync(changelogPath, "utf8").replace(/\r\n/g, "\n").split("\n");
const start = lines.findIndex((line) => header.test(line.trim()));
if (start < 0) {
  throw new Error(`CHANGELOG.md 缺少 ${version} 的版本段落`);
}

let end = lines.length;
for (let i = start + 1; i < lines.length; i += 1) {
  if (/^##\s+\[/.test(lines[i].trim())) {
    end = i;
    break;
  }
}

const body = lines.slice(start, end).join("\n").trim();
if (!body) {
  throw new Error(`CHANGELOG.md 的 ${version} 段落为空`);
}

if (outputPath) {
  fs.mkdirSync(path.dirname(outputPath), { recursive: true });
  fs.writeFileSync(outputPath, `${body}\n`, "utf8");
  console.log(`release notes written to ${outputPath}`);
} else {
  process.stdout.write(`${body}\n`);
}
