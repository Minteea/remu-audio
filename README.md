# Remu Audio

A powerful Rust asynchronous audio playback library with support for local file playback and network streaming.

English | [简体中文](./README.zh-CN.md)

## ✨ Features

- 🎵 **Multiple Format Support** - Supports common audio formats including MP3, WAV, FLAC, OGG, and more
- 🌐 **Network Streaming** - Load and play audio streams from URLs
- ⚡ **Async Loading** - Tokio-based asynchronous downloading and playback
- 🎛️ **Full Control** - Play, pause, seek, volume control, and more
- 📡 **Event-Driven** - Rich event callback system for playback events
- 🔧 **Flexible Extension** - Support for custom Readers and Sources

## 📦 Installation

Add the dependency to your `Cargo.toml`:

```toml
[dependencies]
remu-audio = "0.1.0"
```

## 🚀 Quick Start

### Basic Example

```rust
use remu_audio::player::{Player, PlaybackControl};
use remu_audio::events::PlayerEvent;
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    // Create a player instance
    let mut player = Player::new()?;

    // Set up event callback
    player.set_callback(|event| {
        match event {
            PlayerEvent::Play => println!("Started playing"),
            PlayerEvent::Pause => println!("Paused"),
            PlayerEvent::Ended => println!("Playback ended"),
            _ => {}
        }
    });

    // Load a local file
    player.load_file("audio.mp3").await?;

    // Start playback
    player.play();

    // Wait for playback
    std::thread::sleep(std::time::Duration::from_secs(10));

    Ok(())
}
```

### Network Streaming

```rust
// Load audio from URL
player.load_url("https://example.com/audio.mp3").await?;
player.play();
```

### Playback Control

```rust
// Pause
player.pause();

// Resume playback
player.play();

// Seek to position (20 seconds)
player.seek(Duration::from_secs(20))?;

// Set volume (0.0 - 1.0)
player.set_volume(0.5);

// Get playback state
let is_paused = player.paused();
let position = player.position();
let duration = player.duration();
let volume = player.volume();
```

## 📚 API Documentation

### Player

The main player class providing audio loading and playback functionality.

#### Methods

- `new()` - Create a new player instance
- `load_file(path: &str)` - Load a local audio file
- `load_url(url: &str)` - Load audio from a URL
- `load_reader<R>(reader: R)` - Load from a custom Reader
- `load_source(source: impl Source)` - Load from a Source
- `set_callback<F>(callback: F)` - Set playback event callback
- `set_loader_callback<F>(callback: F)` - Set loader event callback
- `stop()` - Stop playback and clear state
- `ended()` - Check if playback has ended

### PlaybackControl Trait

Provides playback control interface, implemented by `Player` and `PlayerControl`.

#### Methods

- `play()` - Start/resume playback
- `pause()` - Pause playback
- `seek(position: Duration)` - Seek to a specific position
- `set_volume(volume: f32)` - Set volume (0.0 - 1.0)
- `paused()` - Get pause state
- `position()` - Get current playback position
- `duration()` - Get total duration
- `volume()` - Get current volume

### PlayerEvent

Player event enumeration used for event callbacks.

#### Event Types

- `Play` - Playback started or resumed from pause
- `Pause` - Playback paused
- `Playing` - Currently playing (data sufficient)
- `Waiting` - Buffering/waiting for data
- `Ended` - Playback ended
- `Emptied` - Playback content cleared
- `DurationChange` - Duration changed
- `VolumeChange` - Volume changed
- `Seeking` - Seek operation started
- `Seeked` - Seek operation completed
- `LoadStart` - Loading started
- `LoadedData` - Data loaded
- `LoadedMetadata` - Metadata loaded
- `Error { message: String }` - An error occurred

### LoaderEvent

Loader event enumeration for monitoring download status.

#### Event Types

- `Completed` - Download completed
- `Aborted` - Download aborted

## 🎯 Use Cases

### Use Case 1: Music Player

```rust
let mut player = Player::new()?;

// Set up comprehensive event listeners
player.set_callback(|event| {
    match event {
        PlayerEvent::LoadStart => {
            println!("Loading...");
        }
        PlayerEvent::LoadedMetadata => {
            println!("Ready to play");
        }
        PlayerEvent::Play => {
            println!("▶️ Playing");
        }
        PlayerEvent::Pause => {
            println!("⏸️ Paused");
        }
        PlayerEvent::Ended => {
            println!("✅ Playback completed");
        }
        PlayerEvent::Error { message } => {
            eprintln!("❌ Error: {}", message);
        }
        _ => {}
    }
});

player.load_file("song.mp3").await?;
player.play();
```

### Use Case 2: Streaming Media Player

```rust
let mut player = Player::new()?;

// Monitor download progress
player.set_loader_callback(|event| {
    match event {
        LoaderEvent::Completed => {
            println!("✅ Download completed");
        }
        LoaderEvent::Aborted => {
            println!("⚠️ Download aborted");
        }
    }
});

// Load network audio
player.load_url("https://example.com/stream.mp3").await?;
player.play();
```

### Use Case 3: Shared Controller

```rust
let mut player = Player::new()?;
let control = player.control();

// Control playback from another thread
std::thread::spawn(move || {
    let ctrl = control.read().unwrap();
    ctrl.play();
    std::thread::sleep(Duration::from_secs(5));
    ctrl.pause();
});
```

## 🔧 Dependencies

- `rodio` - Audio playback core
- `symphonia` - Audio decoding
- `cpal` - Cross-platform audio I/O
- `tokio` - Async runtime
- `reqwest` - HTTP client
- `anyhow` - Error handling

## 📝 Examples

The project includes complete example code demonstrating various use cases:

```bash
cargo run --example test_playback
```

The example file is located at `examples/test_playback.rs` and includes:

- Local file playback
- Network URL playback
- Playback control (play, pause, seek)
- Volume adjustment
- Event listeners

## 🛠️ Development

### Build the Project

```bash
cargo build
```

### Run Tests

```bash
cargo test
```

### Run Examples

```bash
cargo run --example test_playback
```

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## 🤝 Contributing

Issues and Pull Requests are welcome!

## 📮 Contact

- Project Homepage: https://github.com/Minteea/remu-audio
- Issue Tracker: https://github.com/Minteea/remu-audio/issues

## 🙏 Acknowledgments

Thanks to the following open source projects:

- [rodio](https://github.com/RustAudio/rodio) - Audio playback library
- [symphonia](https://github.com/pdeljanov/Symphonia) - Audio decoding library
- [cpal](https://github.com/RustAudio/cpal) - Cross-platform audio library

## 📃 About README

✨ This README was generated with GitHub Copilot ✨
