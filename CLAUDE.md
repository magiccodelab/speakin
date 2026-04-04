# Voice Input App

Tauri 2 桌面语音输入工具，使用火山引擎豆包大模型流式语音识别 (ASR) 实现实时语音转文字，并自动输入到当前焦点窗口。

## 技术栈

- **后端**: Rust + Tauri 2
- **前端**: React + TypeScript + Tailwind CSS + Vite
- **ASR**: 火山引擎豆包流式语音识别 (WebSocket 二进制协议, `bigmodel_async` 优化版双向流式)
- **音频**: cpal (采集) + rubato (SIMD sinc 重采样) + 自定义 RMS VAD
- **输入模拟**: enigo (键盘模拟) + arboard (剪贴板, by 1Password)
- **热键**: rdev (低级键盘钩子, 支持所有键包括 CapsLock/NumLock)
- **配置**: dotenvy (.env 解析)

## 项目结构

```
src/                          # React 前端
  App.tsx                     # 主组件，状态管理，事件监听
  components/
    VoicePanel.tsx            # 录音按钮 + 音频可视化
    Settings.tsx              # 设置面板
    NetworkLog.tsx            # 网络日志面板

src-tauri/src/                # Rust 后端
  lib.rs                      # Tauri 命令，应用状态，录音生命周期
  asr.rs                      # WebSocket ASR 会话 (核心)
  protocol.rs                 # 火山引擎二进制协议编解码
  audio.rs                    # 麦克风管理，VAD，重采样
  hotkey.rs                   # Windows 全局热键监听
  input.rs                    # 文本输出 (粘贴/打字模拟)
```

## 核心架构

### 录音会话生命周期
1. 热键/按钮触发 → `do_start_recording_impl()`
2. 创建 `(stop_tx, stop_rx)` + `(audio_tx, audio_rx)` 通道
3. `MicrophoneManager::start_forwarding()` 开始转发音频
4. `asr::run_session()` 异步任务：建立 WebSocket → 发送配置 → 音频循环 → 结束包
5. 停止时：先发 `stop_tx` 信号，再 `stop_forwarding()` 关闭音频通道

### ASR 协议要点
- 使用 `bigmodel_async` 端点（优化版双向流式，结果有变化才返回）
- 音频分包：200ms / 6400 bytes (16kHz 16-bit mono)
- 会话结束必须发送 **结束包**（空音频 + `is_last=true` flag），否则服务端 8 秒后超时 (45000081)
- `definite: true` 的 utterance 为已确认文本，其余为 interim

### 前端事件流
- `recording-status` (bool): 录音状态变化
- `transcription-update` (JSON): 转写结果 (`is_final` 区分确认/临时文本)
- `asr-error` (string): ASR 错误
- `connection-status` (bool): WebSocket 连接状态

## 开发规范

- 所有用户可见文本使用中文
- 配置存储在 `.env` 文件
- Rust 代码优先考虑性能，避免不必要的分配
- 前端使用 CSS 变量主题系统 (`--t-fast`, `--t-base` 等过渡时间)
- 录音停止时的关键顺序：stop_tx → stop_forwarding → 状态更新

## 构建

```bash
pnpm install
pnpm tauri dev      # 开发模式
pnpm tauri build    # 生产构建
```

## 常见问题

- **45000081 超时**: 确保结束包正确发送，检查 `asr.rs` 循环退出路径
- **麦克风权限**: Windows 设置 > 隐私和安全 > 麦克风
- **VAD 阈值**: `audio.rs` 中 `threshold: 150.0` (RMS), 静音自动停止 6 秒
