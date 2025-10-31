pub mod downloader;

/// 加载器事件
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoaderEvent {
    /// 下载完成
    Completed,
    /// 下载中断
    Aborted,
}
