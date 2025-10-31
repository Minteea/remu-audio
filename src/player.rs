use anyhow::{Ok, Result};
use cpal::FromSample;
use rodio::mixer::Mixer;
use rodio::Source;
use rodio::{OutputStream, OutputStreamBuilder, Sink};
use std::fs::File;
use std::io::{Read, Seek};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use crate::decoder::Decoder;
use crate::events::PlayerEvent;
use crate::loader::downloader::Downloader;
use crate::reader;

#[allow(dead_code)]
pub struct AudioMetadata {
    title: String,
    artist: String,
    album: String,
}

pub trait PlaybackControl {
    fn play(&self);
    fn pause(&self);
    fn seek(&self, position: Duration) -> Result<(), rodio::source::SeekError>;
    fn set_volume(&self, volume: f32);

    fn paused(&self) -> bool;
    fn duration(&self) -> Option<Duration>;
    fn position(&self) -> Duration;
    fn volume(&self) -> f32;
}

pub struct PlayerControl {
    sink: Sink,
    duration: Option<Duration>,
}

impl PlayerControl {
    fn stop(&self) {
        self.sink.stop();
    }
}

impl PlaybackControl for PlayerControl {
    fn play(&self) {
        self.sink.play();
    }

    fn pause(&self) {
        self.sink.pause();
    }

    fn seek(&self, position: Duration) -> Result<(), rodio::source::SeekError> {
        self.sink.try_seek(position)
    }

    fn set_volume(&self, volume: f32) {
        self.sink.set_volume(volume);
    }

    fn paused(&self) -> bool {
        self.sink.is_paused()
    }

    fn position(&self) -> Duration {
        self.sink.get_pos()
    }

    fn volume(&self) -> f32 {
        self.sink.volume()
    }

    fn duration(&self) -> Option<Duration> {
        self.duration
    }
}

pub struct Player {
    stream: OutputStream,
    control: Arc<RwLock<PlayerControl>>,
    condvar: Option<Arc<Condvar>>,
    cancellation_token: Option<CancellationToken>,
    downloader: Option<Box<Downloader>>,
    /// 回调函数
    callback: Arc<Mutex<Option<Box<dyn Fn(PlayerEvent) + Send + 'static>>>>,
}

impl PlaybackControl for Player {
    fn play(&self) {
        self.control.read().unwrap().play();
        self.emit(PlayerEvent::Play);
    }

    fn pause(&self) {
        self.control.read().unwrap().pause();
        self.emit(PlayerEvent::Pause);
    }

    fn seek(&self, position: Duration) -> Result<(), rodio::source::SeekError> {
        self.emit(PlayerEvent::Seeking);
        let seek_result = self.control.read().unwrap().seek(position);
        if let Err(e) = seek_result {
            return Err(e);
        }
        self.emit(PlayerEvent::Seeked);
        seek_result
    }

    fn set_volume(&self, volume: f32) {
        self.control.read().unwrap().set_volume(volume);
        self.emit(PlayerEvent::VolumeChange);
    }

    fn paused(&self) -> bool {
        self.control.read().unwrap().paused()
    }

    fn position(&self) -> Duration {
        self.control.read().unwrap().position()
    }

    fn volume(&self) -> f32 {
        self.control.read().unwrap().volume()
    }

    fn duration(&self) -> Option<Duration> {
        self.control.read().unwrap().duration()
    }
}

impl Player {
    pub fn new() -> Result<Self> {
        // 创建输出流和sink
        let stream = OutputStreamBuilder::open_default_stream()?;
        let mixer = stream.mixer();
        let sink = Sink::connect_new(&mixer);

        Ok(Self {
            stream,
            control: Arc::new(RwLock::new(PlayerControl {
                sink,
                duration: None,
            })),
            downloader: None,
            condvar: None,
            cancellation_token: None,
            callback: Arc::new(Mutex::new(None)),
        })
    }

    /** 加载音频源 */
    pub fn load_source<S>(&mut self, source: S) -> Result<()>
    where
        S: Source + Send + 'static,
        f32: FromSample<S::Item>,
    {
        // 停止上一个音频播放
        self.control.write().unwrap().stop();

        // 清空相关绑定
        self.clear();

        // 更新时长
        self.control.write().unwrap().duration = source.total_duration();
        self.emit(PlayerEvent::DurationChange);

        // 加载Source
        self.control.write().unwrap().sink.append(source);

        let control = self.control.clone();

        Ok(())
    }

    // 加载本地音频文件
    pub async fn load_file(&mut self, file_path: &str) -> Result<()> {
        // 打开音频文件（支持格式：wav, mp3, flac, ogg等）
        let file = File::open(file_path)?;
        let source = Decoder::try_from(file)?;

        self.load_source(source)?;

        Ok(())
    }

    // 从URL加载音频
    pub async fn load_url(&mut self, url: &str) -> Result<()> {
        let wrapper = crate::reader::MVecBytesWrapper::new(256 * 1024);
        let downloader = Downloader::new(wrapper.clone());
        if let Err(_) = downloader.download(url, None).await {
            return Err(anyhow::anyhow!("Failed to download URL"));
        };
        let reader = reader::MVecBytesReader::new(wrapper, downloader.condvar());

        let cancellation_token = reader.cancellation_token();
        let _ = self.load(reader).unwrap();

        // condvar, downloader, cancellation_token 应在load之后设置，以免被重置
        self.condvar = Some(downloader.condvar());
        self.downloader = Some(Box::new(downloader));
        self.cancellation_token = Some(cancellation_token);

        Ok(())
    }

    // 加载音频源
    pub fn load<R>(&mut self, reader: R) -> Result<()>
    where
        R: Read + Seek + Send + Sync + 'static,
    {
        let source = Decoder::new(reader)?;
        self.load_source(source)
    }

    pub fn mixer(&self) -> &Mixer {
        self.stream.mixer()
    }
    pub fn control(&self) -> Arc<RwLock<PlayerControl>> {
        self.control.clone()
    }

    pub fn handle_message<F>(&self, callback: F)
    where
        F: Fn(PlayerEvent) + Send + 'static,
    {
        let mut cb = self.callback.lock().unwrap();
        *cb = Some(Box::new(callback));
    }

    fn emit(&self, event: PlayerEvent) {
        if let Some(ref cb) = *self.callback.lock().unwrap() {
            cb(event);
        }
    }

    fn stop(&mut self) {
        self.control.read().unwrap().stop();
        self.clear();
        self.emit(PlayerEvent::DurationChange);
    }

    fn clear(&mut self) {
        // 重置控制器
        let mut control = self.control.write().unwrap();
        *control = PlayerControl {
            sink: Sink::connect_new(&self.stream.mixer()),
            duration: None,
        };

        // 清空下载器
        self.downloader = None;

        // 通知Reader取消读取，以免造成阻塞
        if let Some(cancellation_token) = self.cancellation_token.take() {
            println!("通知Reader取消读取");
            cancellation_token.cancel();
        }
        self.cancellation_token = None;

        // 通知Reader所在的播放线程无需等待，以免导致不再使用的播放进程仍然阻塞
        if let Some(condvar) = self.condvar.take() {
            condvar.notify_all();
        }
        self.condvar = None;
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.control.read().unwrap().stop();
        self.clear();
    }
}
