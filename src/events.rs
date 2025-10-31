/// 播放器事件类型
#[derive(Debug, Clone, PartialEq)]
pub enum PlayerEvent {
    /// 播放开始或从暂停恢复（对应 play 事件）
    Play,
    /// 播放暂停（对应 pause 事件）
    Pause,
    /// 正在缓冲/等待数据（对应 waiting 事件）
    Waiting,
    /// 开始播放，数据充足（对应 playing 事件）
    Playing,
    /// 播放结束（对应 ended 事件）
    Ended,
    /// 时长变化（对应 durationchange 事件）
    DurationChange,
    /// 音量变化（对应 volumechange 事件）
    VolumeChange,
    /// Seek 操作开始（对应 seeking 事件）
    Seeking,
    /// Seek 操作完成（对应 seeked 事件）
    Seeked,
    /// 加载开始（对应 loadstart 事件）
    LoadStart,
    /// 数据加载完成（对应 loadeddata 事件）
    LoadedData,
    /// 元数据加载完成（对应 loadedmetadata 事件）
    LoadedMetadata,
    /// 错误发生（对应 error 事件）
    Error { message: String },
}
