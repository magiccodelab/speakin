#!/usr/bin/env node
// Package SpeakIn as a no-install portable ZIP for Windows x64.
//
// Runs AFTER `tauri build --no-bundle`. Stages the release exe and a
// user-facing readme into a temp folder, then compresses with PowerShell
// Compress-Archive (no npm dep, native on Windows 10+).
//
// Output: src-tauri/target/release/bundle/portable/SpeakIn-<version>-portable-x64.zip
//
// ⚠️ This is "no-install portable", NOT "data-portable". The app still
// reads and writes settings / credentials under %APPDATA% and the Windows
// Credential Manager. Users carrying the zip between machines will NOT
// carry their API keys or preferences. The bundled readme explains this
// explicitly so nobody is surprised.

import { execSync } from "node:child_process";
import {
  cpSync,
  existsSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import process from "node:process";

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(__dirname, "..");

// ── Resolve paths ───────────────────────────────────────────────────────────
const pkg = JSON.parse(readFileSync(join(repoRoot, "package.json"), "utf8"));
const version = pkg.version;
const binaryName = "SpeakIn";
const displayName = "SpeakIn声入";

const exePath = join(repoRoot, "src-tauri", "target", "release", "speakin.exe");
if (!existsSync(exePath)) {
  console.error(`✗ Release exe not found: ${exePath}`);
  console.error("  Run `pnpm tauri:build:portable` (which invokes tauri build --no-bundle first) instead of this script directly.");
  process.exit(1);
}

const bundleRoot = join(
  repoRoot,
  "src-tauri",
  "target",
  "release",
  "bundle",
  "portable",
);
mkdirSync(bundleRoot, { recursive: true });

// Staging directory inside the bundle folder. Contents of this dir go
// into the zip at the root level.
const stageName = `${binaryName}-${version}-portable-x64`;
const stageDir = join(bundleRoot, stageName);
// Clean stale staging from previous runs so the zip only reflects this build.
if (existsSync(stageDir)) rmSync(stageDir, { recursive: true, force: true });
mkdirSync(stageDir, { recursive: true });

const zipPath = join(bundleRoot, `${stageName}.zip`);
if (existsSync(zipPath)) rmSync(zipPath, { force: true });

// ── Stage files ─────────────────────────────────────────────────────────────
console.log(`→ staging ${stageName}`);
cpSync(exePath, join(stageDir, `${binaryName}.exe`));

// Bundled readme — sets correct expectations about what "portable" means.
const readme = `${displayName} ${version} — 便携版使用说明
================================================

本版本是「免安装便携版」，解压即可直接运行，不会写入注册表
(除开机自启项)，也不会在开始菜单 / Program Files 留下任何痕迹。

▍ 如何使用
  1. 把整个 ZIP 解压到任意目录 (例如 U 盘、D:\\Apps\\SpeakIn)
  2. 双击 ${binaryName}.exe 启动
  3. 首次启动后在设置里填好 ASR 供应商的 API Key

▍ ⚠️ 数据存储位置 (重要 - 这不是"数据便携")
  本便携版仍会把设置和敏感数据写到 Windows 系统位置:

    • 设置 / 统计 / AI 供应商配置 / 转写历史:
        %APPDATA%\\com.magiccodelab.speakin\\

    • API Key (ASR + AI 供应商):
        Windows 凭据管理器 (控制面板 > 用户账户 > 凭据管理器 >
        Windows 凭据, 查找 "com.magiccodelab.speakin")

    • 开机自启 (如果你在设置里开启了):
        HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run

  这意味着:
    ✓ 在本机内，解压到任何目录都能读到同一份设置
    ✗ 带着 ZIP 换到另一台电脑，需要重新填 API Key 和配置

  如果你需要真正"数据跟着 U 盘走"的版本，请告诉我们:
    https://github.com/magiccodelab/speakIn/issues

▍ 如何彻底清理 (卸载便携版)
  1. 删除解压目录
  2. 删除 %APPDATA%\\com.magiccodelab.speakin\\ 整个文件夹
  3. 打开凭据管理器，删除名称含 "com.magiccodelab.speakin" 的条目
  4. 如果开过自启: 注册表编辑器删除
       HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run
     下名为 "speakin" 的一行

  (安装版卸载器会自动移除开机自启项；只有勾选"删除应用程序数据"时
   才会同时清理上述 2、3。便携版需要手动清理)

▍ 系统要求
  • Windows 10 / 11 (x64)
  • WebView2 Runtime — Windows 11 自带；Windows 10 通常也已随
    Edge 自动安装。若打开后空白，请访问:
    https://developer.microsoft.com/microsoft-edge/webview2/
    手动安装 "Evergreen Bootstrapper"。

▍ 开源协议
  GPL-3.0  —  https://github.com/magiccodelab/speakIn
`;
writeFileSync(join(stageDir, "使用说明.txt"), readme, "utf8");

// ── Compress with PowerShell Compress-Archive ──────────────────────────────
// Using PowerShell avoids adding an npm dependency like archiver/adm-zip.
// Compress-Archive is native on Win10+ and handles unicode file names fine.
// -Path "<stageDir>\*" packs the directory CONTENTS at the zip root so
// extracting yields `SpeakIn.exe` + `使用说明.txt` directly, matching the
// stageName folder's contents.
console.log(`→ compressing → ${zipPath}`);
try {
  const psCommand = [
    "Compress-Archive",
    `-Path '${stageDir.replace(/'/g, "''")}\\*'`,
    `-DestinationPath '${zipPath.replace(/'/g, "''")}'`,
    "-CompressionLevel Optimal",
    "-Force",
  ].join(" ");

  execSync(`powershell -NoProfile -ExecutionPolicy Bypass -Command "${psCommand}"`, {
    stdio: "inherit",
  });
} catch (err) {
  console.error("✗ Compress-Archive failed:", err.message);
  process.exit(1);
}

// Leave staging dir in place — it's the "unzipped" view, useful for QA
// and the zip is small enough that disk duplication is fine.

console.log(`\n✓ portable build ready:`);
console.log(`  ${zipPath}`);
console.log(`  staged copy: ${stageDir}`);
