# Remu Audio

一个功能强大的 Rust 异步音频播放库，支持本地文件播放和网络流媒体播放。

## ✨ 特性

- 🎵 **多格式支持** - 支持 MP3、WAV、FLAC、OGG 等常见音频格式
- 🌐 **网络流播放** - 支持从 URL 加载和播放音频流
- ⚡ **异步加载** - 基于 Tokio 的异步下载和播放
- 🎛️ **完整控制** - 播放、暂停、跳转、音量控制等功能
- 📡 **事件驱动** - 丰富的播放事件回调系统
- 🔧 **灵活扩展** - 支持自定义 Reader 和 Source

## 📦 安装

在 `Cargo.toml` 中添加依赖：

```toml
[dependencies]
remu-audio = "0.1.0"
```

## 🚀 快速开始

### 基础示例

```rust
use remu_audio::player::{Player, PlaybackControl};
use remu_audio::events::PlayerEvent;
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    // 创建播放器实例
    let mut player = Player::new()?;

    // 设置事件回调
    player.set_callback(|event| {
        match event {
            PlayerEvent::Play => println!("开始播放"),
            PlayerEvent::Pause => println!("播放暂停"),
            PlayerEvent::Ended => println!("播放结束"),
            _ => {}
        }
    });

    // 加载本地文件
    player.load_file("audio.mp3").await?;

    // 开始播放
    player.play();

    // 等待播放完成
    std::thread::sleep(std::time::Duration::from_secs(10));

    Ok(())
}
```

### 网络流播放

```rust
// 从 URL 加载音频
player.load_url("https://example.com/audio.mp3").await?;
player.play();
```

### 播放控制

```rust
// 暂停
player.pause();

// 继续播放
player.play();

// 跳转到指定位置（20 秒）
player.seek(Duration::from_secs(20))?;

// 设置音量（0.0 - 1.0）
player.set_volume(0.5);

// 获取播放状态
let is_paused = player.paused();
let position = player.position();
let duration = player.duration();
let volume = player.volume();
```

## 📚 API 文档

### Player

主要的播放器类，提供音频加载和播放功能。

#### 方法

- `new()` - 创建新的播放器实例
- `load_file(path: &str)` - 加载本地音频文件
- `load_url(url: &str)` - 从 URL 加载音频
- `load_reader<R>(reader: R)` - 从自定义 Reader 加载
- `load_source(source: impl Source)` - 从 Source 加载
- `set_callback<F>(callback: F)` - 设置播放事件回调
- `set_loader_callback<F>(callback: F)` - 设置加载事件回调
- `stop()` - 停止播放并清空状态
- `ended()` - 检查是否播放完成

### PlaybackControl Trait

提供播放控制接口，由 `Player` 和 `PlayerControl` 实现。

#### 方法

- `play()` - 开始/继续播放
- `pause()` - 暂停播放
- `seek(position: Duration)` - 跳转到指定位置
- `set_volume(volume: f32)` - 设置音量（0.0 - 1.0）
- `paused()` - 获取暂停状态
- `position()` - 获取当前播放位置
- `duration()` - 获取总时长
- `volume()` - 获取当前音量

### PlayerEvent

播放器事件枚举，用于事件回调。

#### 事件类型

- `Play` - 播放开始或从暂停恢复
- `Pause` - 播放暂停
- `Playing` - 正在播放（数据充足）
- `Waiting` - 正在缓冲/等待数据
- `Ended` - 播放结束
- `Emptied` - 播放内容被清空
- `DurationChange` - 时长变化
- `VolumeChange` - 音量变化
- `Seeking` - 跳转操作开始
- `Seeked` - 跳转操作完成
- `LoadStart` - 开始加载
- `LoadedData` - 数据加载完成
- `LoadedMetadata` - 元数据加载完成
- `Error { message: String }` - 发生错误

### LoaderEvent

加载器事件枚举，用于监听下载状态。

#### 事件类型

- `Completed` - 下载完成
- `Aborted` - 下载中止

## 🎯 使用场景

### 场景 1：音乐播放器

```rust
let mut player = Player::new()?;

// 设置完整的事件监听
player.set_callback(|event| {
    match event {
        PlayerEvent::LoadStart => {
            println!("正在加载...");
        }
        PlayerEvent::LoadedMetadata => {
            println!("准备就绪");
        }
        PlayerEvent::Play => {
            println!("▶️ 播放");
        }
        PlayerEvent::Pause => {
            println!("⏸️ 暂停");
        }
        PlayerEvent::Ended => {
            println!("✅ 播放完成");
        }
        PlayerEvent::Error { message } => {
            eprintln!("❌ 错误: {}", message);
        }
        _ => {}
    }
});

player.load_file("song.mp3").await?;
player.play();
```

### 场景 2：流媒体播放

```rust
let mut player = Player::new()?;

// 监听下载进度
player.set_loader_callback(|event| {
    match event {
        LoaderEvent::Completed => {
            println!("✅ 下载完成");
        }
        LoaderEvent::Aborted => {
            println!("⚠️ 下载中止");
        }
    }
});

// 加载网络音频
player.load_url("https://example.com/stream.mp3").await?;
player.play();
```

### 场景 3：多控制器共享

```rust
let mut player = Player::new()?;
let control = player.control();

// 在其他线程中控制播放
std::thread::spawn(move || {
    let ctrl = control.read().unwrap();
    ctrl.play();
    std::thread::sleep(Duration::from_secs(5));
    ctrl.pause();
});
```

## 🔧 依赖项

- `rodio` - 音频播放核心
- `symphonia` - 音频解码
- `cpal` - 跨平台音频 I/O
- `tokio` - 异步运行时
- `reqwest` - HTTP 客户端
- `anyhow` - 错误处理

## 📝 示例

项目包含完整的示例代码，展示了各种使用场景：

```bash
cargo run --example test_playback
```

示例文件位于 `examples/test_playback.rs`，包含：

- 本地文件播放
- 网络 URL 播放
- 播放控制（播放、暂停、跳转）
- 音量调整
- 事件监听

## 🛠️ 开发

### 构建项目

```bash
cargo build
```

### 运行测试

```bash
cargo test
```

### 运行示例

```bash
cargo run --example test_playback
```

## 📄 许可证

本项目采用 MIT 许可证 - 详见 [LICENSE](LICENSE) 文件。

## 🤝 贡献

欢迎提交 Issue 和 Pull Request！

## 📮 联系方式

- 项目主页: https://github.com/Minteea/remu-audio
- 问题反馈: https://github.com/Minteea/remu-audio/issues

## 🙏 致谢

感谢以下开源项目：

- [rodio](https://github.com/RustAudio/rodio) - 音频播放库
- [symphonia](https://github.com/pdeljanov/Symphonia) - 音频解码库
- [cpal](https://github.com/RustAudio/cpal) - 跨平台音频库

## 📃 关于 README

✨ 本 README 使用 Github Copilot 生成 ✨
