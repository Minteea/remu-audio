use anyhow::{Ok, Result};
use cpal::FromSample;
use rodio::mixer::Mixer;
use rodio::source::EmptyCallback;
use rodio::Source;
use rodio::{OutputStream, OutputStreamBuilder, Sink};
use std::fs::File;
use std::io::{Read, Seek};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, RwLock};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use crate::decoder::Decoder;
use crate::events::PlayerEvent;
use crate::loader::downloader::Downloader;
use crate::loader::LoaderEvent;
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
    loader: Option<Box<Downloader>>,
    /// 回调函数
    callback: Arc<RwLock<Option<Box<dyn Fn(PlayerEvent) + Send + Sync + 'static>>>>,
    loader_callback: Arc<RwLock<Option<Box<dyn Fn(LoaderEvent) + Send + Sync + 'static>>>>,
    empty: Arc<AtomicBool>,
    ended: Arc<AtomicBool>,
    autoplay: Arc<AtomicBool>,
}

impl PlaybackControl for Player {
    fn play(&self) {
        self.control.read().unwrap().play();
        self.autoplay.store(true, Ordering::SeqCst);
        self.emit(PlayerEvent::Play);
    }

    fn pause(&self) {
        self.control.read().unwrap().pause();
        self.autoplay.store(false, Ordering::SeqCst);
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
        sink.pause();

        Ok(Self {
            stream,
            control: Arc::new(RwLock::new(PlayerControl {
                sink,
                duration: None,
            })),
            loader: None,
            condvar: None,
            cancellation_token: None,
            callback: Arc::new(RwLock::new(None)),
            loader_callback: Arc::new(RwLock::new(None)),
            empty: Arc::new(AtomicBool::new(true)),
            ended: Arc::new(AtomicBool::new(false)),
            autoplay: Arc::new(AtomicBool::new(false)),
        })
    }

    /** 加载音频源 */
    fn load<S>(&mut self, source: S) -> Result<()>
    where
        S: Source + Send + 'static,
        f32: FromSample<S::Item>,
    {
        // 清空相关绑定
        if !self.empty() {
            self.clear();
        }
        self.empty.store(false, Ordering::SeqCst);

        // 更新时长
        self.control.write().unwrap().duration = source.total_duration();
        self.emit(PlayerEvent::DurationChange);
        self.emit(PlayerEvent::LoadedMetadata);
        self.emit(PlayerEvent::LoadedData);

        // 加载Source
        let control = self.control.write().unwrap();
        control.sink.append(source);

        let callback = self.callback.clone();
        let ended = self.ended.clone();
        control.sink.append(EmptyCallback::new(Box::new(move || {
            if let Some(ref cb) = *callback.read().unwrap() {
                ended.store(true, Ordering::SeqCst);
                cb(PlayerEvent::Ended);
            }
        })));

        Ok(())
    }

    // 加载本地音频文件
    pub async fn load_file(&mut self, file_path: &str) -> Result<()> {
        // 清空相关绑定
        if !self.empty() {
            self.clear();
        }

        // 打开音频文件（支持格式：wav, mp3, flac, ogg等）
        let file = File::open(file_path)?;
        let source = Decoder::try_from(file)?;

        self.load(source)?;

        Ok(())
    }

    // 从URL加载音频
    pub async fn load_url(&mut self, url: &str) -> Result<()> {
        // 清空相关绑定
        if !self.empty() {
            self.clear();
        }

        let wrapper = crate::reader::MVecBytesWrapper::new(256 * 1024);
        let loader = Downloader::new(wrapper.clone());

        let loader_callback = self.loader_callback.clone();
        loader.set_callback(move |event| {
            if let Some(ref cb) = *loader_callback.read().unwrap() {
                cb(event);
            }
        });
        if let Err(_) = loader.download(url, None).await {
            self.emit(PlayerEvent::Error {
                message: "Failed to download URL".into(),
            });
            return Err(anyhow::anyhow!("Failed to download URL"));
        };
        let reader = reader::MVecBytesReader::new(wrapper, loader.condvar());

        let cancellation_token = reader.cancellation_token();

        let source = Decoder::new(reader)?;
        if let Err(e) = self.load(source) {
            return Err(e);
        }

        // condvar, loader, cancellation_token 应在load之后设置，以免被重置
        self.condvar = Some(loader.condvar());
        self.loader = Some(Box::new(loader));
        self.cancellation_token = Some(cancellation_token);

        Ok(())
    }

    // 从Reader加载音频
    pub fn load_reader<R>(&mut self, reader: R) -> Result<()>
    where
        R: Read + Seek + Send + Sync + 'static,
    {
        // 清空相关绑定
        if !self.empty() {
            self.clear();
        }

        let source = Decoder::new(reader)?;
        self.load(source)
    }

    // 从Source加载音频
    pub fn load_source(&mut self, source: impl Source + Send + 'static) -> Result<()> {
        // 清空相关绑定
        if !self.empty() {
            self.clear();
        }

        self.emit(PlayerEvent::LoadStart);

        self.load(source)
    }

    pub fn mixer(&self) -> &Mixer {
        self.stream.mixer()
    }
    pub fn control(&self) -> Arc<RwLock<PlayerControl>> {
        self.control.clone()
    }

    pub fn set_callback<F>(&self, callback: F)
    where
        F: Fn(PlayerEvent) + Send + Sync + 'static,
    {
        let mut cb = self.callback.write().unwrap();
        *cb = Some(Box::new(callback));
    }

    pub fn set_loader_callback<F>(&self, callback: F)
    where
        F: Fn(LoaderEvent) + Send + Sync + 'static,
    {
        let mut cb = self.loader_callback.write().unwrap();
        *cb = Some(Box::new(callback));
    }

    fn emit(&self, event: PlayerEvent) {
        if let Some(ref cb) = *self.callback.read().unwrap() {
            cb(event);
        }
    }

    /// 清空播放状态
    pub fn stop(&mut self) {
        self.clear();
    }

    /// 强制清除正在播放的资源
    fn clear(&mut self) {
        // 停止播放
        self.control.read().unwrap().stop();

        // 重置控制器
        let mut control = self.control.write().unwrap();
        let previous_duration = control.duration.take();
        let sink = Sink::connect_new(&self.stream.mixer());
        if !(self.autoplay.load(Ordering::SeqCst)) {
            sink.pause();
        }
        *control = PlayerControl {
            sink,
            duration: None,
        };
        drop(control);

        // 清空下载器
        self.loader = None;

        // 通知Reader取消读取，以免造成阻塞
        if let Some(cancellation_token) = self.cancellation_token.take() {
            cancellation_token.cancel();
        }
        self.cancellation_token = None;

        // 通知Reader所在的播放线程无需等待，以免导致不再使用的播放进程仍然阻塞
        if let Some(condvar) = self.condvar.take() {
            condvar.notify_all();
        }
        self.condvar = None;

        self.ended.store(false, Ordering::SeqCst);

        if !self.empty() {
            // 标记为已清空，发送清空事件
            self.empty.store(true, Ordering::SeqCst);
            self.emit(PlayerEvent::Emptied);
        }

        // 如果之前有播放时长，发送时长变化事件
        if previous_duration != None {
            self.emit(PlayerEvent::DurationChange);
        }
    }

    fn empty(&self) -> bool {
        self.empty.load(Ordering::SeqCst)
    }

    pub fn ended(&self) -> bool {
        self.ended.load(Ordering::SeqCst)
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.clear();
    }
}
