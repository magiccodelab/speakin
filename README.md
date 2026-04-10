<div align="center">

# SpeakIn声入

**静默运行的桌面语音输入工具 · 按下热键即说即输**

[![License: GPL v3](https://img.shields.io/badge/License-GPL_v3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)
[![Platform](https://img.shields.io/badge/platform-Windows-lightgrey.svg)](#)
[![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB.svg)](https://tauri.app)
[![Rust](https://img.shields.io/badge/Rust-1.75+-orange.svg)](https://www.rust-lang.org)

</div>

---

> **📌 平台说明：** 当前版本主要适配 **Windows**，已提供开箱即用的安装包。macOS / Linux 用户需从源码自行构建，部分平台相关功能（全局热键、WASAPI Loopback 等）可能需要修改代码适配。项目采用模块化架构，平台相关代码集中在少数几个文件，维护和移植成本较低——推荐使用 [Claude Code](https://code.claude.com/docs) 或 [Codex](https://openai.com/index/codex/) 等 AI 编程工具辅助构建。

## 简介

SpeakIn（声入）是一款面向 **超级个体、独立开发者、内容创作者** 的桌面语音输入工具。它在后台静默运行，通过全局热键触发录音，语音识别后自动将文字输入到当前焦点窗口（IDE、浏览器、聊天框、任意文本编辑器）。

**为什么做这个工具？**

每天向 AI 输出大量想法（需求描述、架构设计、Bug 分析）动辄上千字，打字不仅慢，还持续损耗腱鞘和精力。语音输入可以轻松达到 **300+ 字 / 分钟**，说话更像在给 AI 下指令，而不是在编辑文本——更容易进入心流状态。

## 核心特性

- 🎤 **全局热键触发**：按下说话，松开或再按一次即停；无需切换窗口
- ⚡ **即时输入**：识别完成立即粘贴或键入到当前焦点位置
- 🌐 **多供应商支持**：豆包（火山引擎）/ 百炼（DashScope）/ 千问（Realtime API），按需切换
- 💰 **零成本日常使用**：利用各家云 ASR 的免费额度，日常使用基本不花钱
- 🤖 **AI 优化润色**：可选 OpenAI 兼容 API 做润色、翻译、重写（中文说话 → 英文输出）
- 🎧 **双音频源**：支持麦克风输入 + 系统声音捕获（会议/视频转录）
- 📊 **可视化反馈**：独立的系统级覆盖层显示录音状态与实时字幕
- 🔒 **本地优先**：配置和凭据存储在本地（JSON + OS Keyring），不上传任何服务器
- 🌓 **主题与动效**：深色/浅色主题、可自定义主题色

## 截图

> 首次发布版本的界面截图将在 v1.0.0 Release 页面提供

## 安装

### Windows（推荐）

前往 [Releases](https://github.com/magiccodelab/speakIn/releases) 页面下载最新的 `.msi` 或 `.exe` 安装包。

> **⚠️ 关于 Windows SmartScreen 警告**
>
> 由于 SpeakIn 是开源项目暂未进行代码签名，首次运行安装包时 Windows Defender SmartScreen 可能会提示 "Windows 已保护你的电脑"。这是未签名应用的正常提示，**并非病毒**。
>
> 解决方法：点击 **"更多信息"** → **"仍要运行"** 即可。源代码完全公开，你可以自行审计或从源码构建。

安装器默认以 **per-user** 模式安装到用户目录，**无需管理员权限**。

### 从源码构建

详见 [构建](#构建) 章节。

## 快速开始

### 1. 获取 ASR API 凭据

SpeakIn 目前支持三家国内云 ASR，任选其一（均有免费额度）：

| 供应商 | 文档 | 免费额度 |
|---|---|---|
| **豆包** (火山引擎) | [控制台](https://console.volcengine.com/speech/service/10011) | 新用户赠送 |
| **百炼** (DashScope) | [控制台](https://dashscope.console.aliyun.com/) | 有免费额度 |
| **千问 Realtime** | [控制台](https://bailian.console.aliyun.com/) | 有免费额度 |

### 2. 配置 SpeakIn

1. 首次启动后按引导完成初始化
2. 打开 **设置 → ASR** 选择供应商，填入 API Key / App ID 等凭据
3. 可选：**设置 → AI 优化** 配置 OpenAI 兼容供应商与提示词
4. **设置 → 快捷键** 自定义全局热键（默认：见设置面板）

### 3. 开始使用

- **按下热键** 开始录音，覆盖层会显示实时识别字幕
- **松开** 或 **再次按下** 结束录音（行为可在设置中切换）
- 转写结果会自动输入到当前焦点窗口
- 如启用 AI 优化，会先经过模型润色再输出

## 技术栈

- **后端**: Rust + Tauri 2
- **前端**: React 19 + TypeScript + Tailwind CSS 4 + Vite
- **音频**: cpal (麦克风) + WASAPI Loopback (系统声音) + rubato (重采样) + 自定义 RMS VAD
- **ASR**: tokio-tungstenite WebSocket 直连各供应商协议
- **输入**: enigo (键盘模拟) + arboard (剪贴板)
- **热键**: windows-sys 原生低级键盘钩子
- **存储**: tauri-plugin-store (JSON) + OS Keyring (凭据)

## 构建

### 依赖

- [Rust](https://www.rust-lang.org) 1.75+
- [Node.js](https://nodejs.org) 22+ 和 [pnpm](https://pnpm.io)
- [Tauri 2 环境依赖](https://tauri.app/start/prerequisites/)
- Windows 10/11（主要适配平台）；macOS / Linux 需自行适配部分平台相关代码

### 命令

```bash
# 安装依赖
pnpm install

# 开发模式（热重载）
pnpm tauri dev

# 生产构建（输出至 src-tauri/target/release/bundle/）
pnpm tauri:build

# 生产构建 + sccache 缓存加速（需提前装好 sccache）
pnpm tauri:build:cached
```

构建配置细节见 `src-tauri/.cargo/config.toml`（启用了 `rust-lld` 链接器和 jobs=16）。

## 项目结构

```
src/                              # React 前端
  App.tsx                         # 主组件，状态管理
  components/
    VoicePanel.tsx                # 录音按钮 + 音频波形
    Settings.tsx                  # 设置面板
    RecordingOverlay.tsx          # 独立的系统级覆盖层窗口
    settings/                     # 各设置 Tab
  lib/                            # 工具与类型定义

src-tauri/src/                    # Rust 后端
  lib.rs                          # Tauri 命令与应用状态
  asr/                            # ASR 供应商抽象层
    doubao.rs / dashscope.rs / qwen.rs
  ai/                             # AI 优化模块
  audio.rs                        # 麦克风 + VAD + 重采样
  loopback.rs                     # WASAPI Loopback
  hotkey.rs                       # 全局热键
  input.rs                        # 文本输出
  storage.rs                      # 配置持久化
```

## 常见问题

**Q: 热键没反应？**
A: 检查设置里的快捷键是否与其他软件冲突（如输入法、截图工具）。

**Q: 麦克风没声音？**
A: 确认 Windows 设置 → 隐私和安全 → 麦克风 中已授权 SpeakIn。

**Q: 系统声音捕获无输出？**
A: 确认默认输出设备有音频正在播放，WASAPI Loopback 需要活跃的音频流。

**Q: ASR 偶尔连接失败怎么办？**
A: 直接再按一次热键重录。国内主流云 ASR SLA > 99.99%，偶发失败通常是本地网络瞬时抖动。

**Q: 为什么不做代码签名？**
A: OV 代码签名证书费用高昂且需商业主体。项目正在申请开源项目免费签名（SignPath.io OSS 计划），完成后会在 Release 页面说明。

## 路线图

- [x] 多供应商 ASR 支持（豆包 / 百炼 / 千问）
- [x] 系统声音捕获
- [x] AI 优化与自定义提示词
- [x] 独立覆盖层窗口与历史记录
- [ ] macOS 适配
- [ ] 更多 ASR 供应商（OpenAI Whisper API、本地 Whisper）
- [ ] 自动更新机制

## 贡献

欢迎提交 Pull Request。提交 PR 前请确保：
- `pnpm tauri dev` 能正常启动且功能测试通过
- Rust 代码通过 `cargo fmt` 和 `cargo clippy`
- 新增的用户可见文本为简体中文

## 开源协议

本项目采用 [GPL-3.0](./LICENSE) 协议开源。这意味着：

- ✅ 你可以自由使用、修改、分发本软件
- ✅ 你可以将其用于商业用途
- ⚠️ 如果你分发修改版，必须同样以 GPL-3.0 开源
- ⚠️ 必须保留原作者署名与协议声明

## 致谢

- [Tauri](https://tauri.app) - 优秀的跨平台桌面框架
- [cpal](https://github.com/RustAudio/cpal) - Rust 音频输入输出库
- [enigo](https://github.com/enigo-rs/enigo) - 跨平台输入模拟
- 豆包、百炼、千问等云端 ASR 服务提供商

---

<div align="center">

**让语音成为最自然的输入方式**

Made with ❤️ by [magiccodelab](https://github.com/magiccodelab)

</div>
