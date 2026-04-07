# Changelog

All notable changes to SpeakIn will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0] - 2026-04-07

首个公开发布版本。

### Added

- **全局热键语音输入**：低级键盘钩子捕获自定义热键，按下即说、松开即停
- **多供应商 ASR 支持**：
  - 豆包（火山引擎二进制协议，支持 BiStream / NoStream 两种模式）
  - 百炼（DashScope 双工流式）
  - 千问（阿里云 Realtime API）
- **双音频源**：麦克风输入 + WASAPI Loopback 系统声音捕获
- **智能 VAD**：
  - RMS 阈值过滤环境噪音
  - 30 秒无语音自动取消（不连接 ASR，零计费）
  - 检测到语音后 6 秒静音自动停止
  - 预缓冲 + trailing 避免首尾截断
- **AI 优化模块**：OpenAI 兼容 API 客户端，支持多供应商、自定义提示词、流式输出
- **文本输入**：粘贴模式 + 键入模拟双策略，可选粘贴后恢复剪贴板
- **系统级覆盖层**：独立透明窗口显示录音状态与实时字幕，可拖动、位置持久化
- **设置面板**：
  - ASR 供应商与音频来源
  - AI 优化与提示词管理
  - 使用统计
  - 主题与快捷键自定义
- **历史记录**：最近转录记录查看与复制
- **多语言安装包**：NSIS / WiX 同时支持简体中文与英文
- **系统托盘**：最小化到托盘、右键菜单快捷操作
- **深色模式**：跟随系统或手动切换，自定义主题色

### Technical

- Rust + Tauri 2 + React 19 + TypeScript + Tailwind CSS 4
- Release 构建启用 LTO、单 codegen unit、符号裁剪
- 支持 `rust-lld` 链接器加速 + `sccache` 可选缓存
- 凭据通过 OS Keyring 安全存储，配置走 tauri-plugin-store

### Known Issues

- **Windows SmartScreen 警告**：安装包暂未进行代码签名，首次运行会触发"未知发行者"提示，点击"更多信息 → 仍要运行"即可
- **平台支持**：当前仅支持 Windows 10/11，macOS 与 Linux 尚未适配
- **网络瞬时错误**：极少数情况下 ASR WebSocket 连接可能瞬时失败，手动重试即可

[1.0.0]: https://github.com/magiccodelab/speakIn/releases/tag/v1.0.0
