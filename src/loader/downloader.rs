use std::sync::Condvar;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use std::thread;

use crate::reader::AppendableDataWrapper;

/// 下载状态枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadStatus {
    /// 未开始下载
    NotStarted,
    /// 下载中
    Downloading,
    /// 下载完成
    Completed,
    /// 下载中断
    Aborted,
}

/// 下载事件枚举，用于回调函数
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadEvent {
    /// 获取到Header
    HeaderReceived,
    /// 下载完成
    Completed,
    /// 下载中断
    Aborted,
}

/// 下载器结构体
pub struct Downloader {
    /// 下载的数据
    data: Arc<Mutex<Box<dyn AppendableDataWrapper + Send + 'static>>>,
    /// 条件变量(每获取一次数据触发一次)
    condvar: Arc<Condvar>,
    /// 下载状态
    status: Arc<Mutex<DownloadStatus>>,
    /// 文件总字节数
    total_bytes: Arc<AtomicU64>,
    /// 已下载字节数
    downloaded_bytes: Arc<AtomicU64>,
    /// 是否已经调用过download方法
    download_called: Arc<AtomicBool>,
    /// 是否需要中断下载
    should_abort: Arc<AtomicBool>,
    /// 下载是否已完成（用于通知Reader停止等待）
    download_completed: Arc<AtomicBool>,
    /// 下载线程句柄
    thread_handle: Arc<Mutex<Option<tokio::task::JoinHandle<Result<(), ()>>>>>,
    /// 回调函数
    callback: Arc<Mutex<Option<Box<dyn Fn(DownloadEvent) + Send + 'static>>>>,
}

impl Downloader {
    /// 创建新的下载器实例
    pub fn new<T: AppendableDataWrapper + Send + 'static>(data: T) -> Self {
        Self {
            data: Arc::new(Mutex::new(Box::new(data))),
            condvar: Arc::new(Condvar::new()),
            status: Arc::new(Mutex::new(DownloadStatus::NotStarted)),
            total_bytes: Arc::new(AtomicU64::new(0)),
            downloaded_bytes: Arc::new(AtomicU64::new(0)),
            download_called: Arc::new(AtomicBool::new(false)),
            should_abort: Arc::new(AtomicBool::new(false)),
            download_completed: Arc::new(AtomicBool::new(false)),
            thread_handle: Arc::new(Mutex::new(None)),
            callback: Arc::new(Mutex::new(None)),
        }
    }

    /// 获取当前下载状态
    pub fn status(&self) -> DownloadStatus {
        *self.status.lock().unwrap()
    }

    /// 获取文件总字节数
    pub fn total_bytes(&self) -> u64 {
        self.total_bytes.load(Ordering::Relaxed)
    }

    /// 获取已下载字节数
    pub fn downloaded_bytes(&self) -> u64 {
        self.downloaded_bytes.load(Ordering::Relaxed)
    }

    /// 获取下载数据的引用
    pub fn data(&self) -> Arc<Mutex<Box<dyn AppendableDataWrapper + Send + 'static>>> {
        Arc::clone(&self.data)
    }

    /// 获取条件变量的引用
    pub fn condvar(&self) -> Arc<Condvar> {
        Arc::clone(&self.condvar)
    }

    /// 获取下载完成标志的引用
    pub fn download_completed(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.download_completed)
    }

    /// 设置消息回调函数
    ///
    /// # 参数
    /// * `callback` - 回调函数，接收DownloadEvent作为参数
    ///
    /// # 注意
    /// 多次调用会替换之前设置的回调函数
    pub fn handle_message<F>(&self, callback: F)
    where
        F: Fn(DownloadEvent) + Send + 'static,
    {
        let mut cb = self.callback.lock().unwrap();
        *cb = Some(Box::new(callback));
    }

    /// 开始下载
    ///
    /// # 参数
    /// * `url` - 下载地址
    /// * `headers` - 可选的HTTP请求头
    ///
    /// # 返回
    /// * `Ok(())` - 下载请求成功，获取到数据
    /// * `Err(())` - 下载请求失败
    ///
    /// # Panics
    /// 如果多次调用此方法会触发panic
    pub async fn download(
        &self,
        url: &str,
        headers: Option<Vec<(String, String)>>,
    ) -> Result<(), ()> {
        // 检查是否已经调用过download
        if self.download_called.swap(true, Ordering::SeqCst) {
            panic!("download() can only be called once");
        }

        // 更新状态为下载中
        {
            let mut status = self.status.lock().unwrap();
            *status = DownloadStatus::Downloading;
        }

        // 克隆需要在线程中使用的Arc引用
        let data = Arc::clone(&self.data);
        let condvar = Arc::clone(&self.condvar);
        let status = Arc::clone(&self.status);
        let total_bytes = Arc::clone(&self.total_bytes);
        let downloaded_bytes = Arc::clone(&self.downloaded_bytes);
        let should_abort = Arc::clone(&self.should_abort);
        let download_completed = Arc::clone(&self.download_completed);
        let callback = Arc::clone(&self.callback);

        use futures_util::StreamExt;

        // 构建HTTP客户端和请求
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap();

        let mut request_builder = client.get(url);

        // 添加自定义headers
        if let Some(hdrs) = headers {
            for (key, value) in hdrs {
                request_builder = request_builder.header(key, value);
            }
        }

        // 发送请求
        let response = match request_builder.send().await {
            Ok(resp) => resp,
            Err(e) => {
                eprintln!("Failed to send request: {}", e);
                let mut s = status.lock().unwrap();
                *s = DownloadStatus::Aborted;
                if let Some(ref cb) = *callback.lock().unwrap() {
                    cb(DownloadEvent::Aborted);
                }
                return Err(());
            }
        };

        // 获取Content-Length
        let content_length = response
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);

        total_bytes.store(content_length, Ordering::Relaxed);

        // 设置数据容量，以防内存重新分配导致卡顿
        data.lock().unwrap().set_capacity(content_length as usize);

        // 触发HeaderReceived回调
        if let Some(ref cb) = *callback.lock().unwrap() {
            cb(DownloadEvent::HeaderReceived);
        }

        // 创建流式下载线程
        let handle = tokio::task::spawn(async move {
            // 使用真正的流式下载
            let mut stream = response.bytes_stream();

            while let Some(chunk_result) = stream.next().await {
                // 检查是否需要中断
                if should_abort.load(Ordering::Relaxed) {
                    let mut s = status.lock().unwrap();
                    *s = DownloadStatus::Aborted;
                    if let Some(ref cb) = *callback.lock().unwrap() {
                        cb(DownloadEvent::Aborted);
                    }
                    return Err(());
                }

                match chunk_result {
                    Ok(chunk) => {
                        // 将数据追加到data中
                        let mut data_lock = data.lock().unwrap();
                        data_lock.append_data(&chunk);
                        drop(data_lock);
                        // 获取到数据后，解除Reader对缓冲区数据的等待
                        condvar.notify_all();

                        // 更新已下载字节数
                        downloaded_bytes.fetch_add(chunk.len() as u64, Ordering::Relaxed);
                    }
                    Err(e) => {
                        eprintln!("Error reading chunk: {}", e);
                        let mut s = status.lock().unwrap();
                        *s = DownloadStatus::Aborted;
                        if let Some(ref cb) = *callback.lock().unwrap() {
                            cb(DownloadEvent::Aborted);
                        }
                        return Err(());
                    }
                }
            }

            data.lock().unwrap().complete();

            // 下载完成
            let mut s = status.lock().unwrap();
            *s = DownloadStatus::Completed;

            // 设置下载完成标志，并通知所有等待的Reader
            download_completed.store(true, Ordering::Release);
            condvar.notify_all();

            if let Some(ref cb) = *callback.lock().unwrap() {
                cb(DownloadEvent::Completed);
            }
            println!("downloader / 当前线程 ID: {:?}", thread::current().id());
            println!("下载完成");
            return Ok(());
        });

        let mut th = self.thread_handle.lock().unwrap();
        *th = Some(handle);
        Ok(())
    }

    /// 中断当前下载
    pub fn abort(&self) -> Result<(), DownloadStatus> {
        let mut status = self.status.lock().unwrap();
        if *status != DownloadStatus::Downloading {
            return Err(status.clone());
        }
        // 设置中断标志
        self.should_abort.store(true, Ordering::SeqCst);

        // 中断下载线程
        let mut th = self.thread_handle.lock().unwrap();
        if let Some(handle) = th.take() {
            let _ = handle.abort();
        }
        *status = DownloadStatus::Aborted;
        Ok(())
    }
}

impl Drop for Downloader {
    fn drop(&mut self) {
        // 中断下载
        let mut status = self.status.lock().unwrap();
        // 设置中断标志
        self.should_abort.store(true, Ordering::SeqCst);

        // 中断下载线程
        let mut th = self.thread_handle.lock().unwrap();
        if let Some(handle) = th.take() {
            let _ = handle.abort();
        }
        *status = DownloadStatus::Aborted;
    }
}
