use std::io::{Read, Result, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use tokio_util::sync::CancellationToken;

use crate::reader::AppendableDataWrapper;

#[derive(Debug, Clone)]
pub struct MVecU8Wrapper {
    data: Arc<Mutex<Vec<u8>>>,
    completed: Arc<AtomicBool>,
}

impl MVecU8Wrapper {
    pub fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(Vec::new())),
            completed: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn data(&self) -> Arc<Mutex<Vec<u8>>> {
        self.data.clone()
    }
    pub fn completed(&self) -> Arc<AtomicBool> {
        self.completed.clone()
    }
}

impl AppendableDataWrapper for MVecU8Wrapper {
    fn append_data(&mut self, slice: &[u8]) {
        // 将数据追加到data中
        let mut data_lock = self.data.lock().unwrap();
        data_lock.extend_from_slice(slice);
    }
    fn complete(&mut self) {
        self.completed.store(true, Ordering::SeqCst);
    }
}

pub struct MVecU8Reader {
    data: Arc<Mutex<Vec<u8>>>,
    condvar: Arc<Condvar>,
    pos: u64,
    download_completed: Arc<AtomicBool>,
    cancellation_token: CancellationToken,
}

impl MVecU8Reader {
    pub fn new(wrapper: MVecU8Wrapper, condvar: Arc<Condvar>) -> Self {
        Self {
            data: wrapper.data(),
            condvar,
            pos: 0,
            download_completed: wrapper.completed(),
            cancellation_token: CancellationToken::new(),
        }
    }

    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation_token.clone()
    }
}

impl Read for MVecU8Reader {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let lock = &*self.data;
        let mut data = lock.lock().unwrap();

        // 如果需要读取的数据位置超出当前缓冲区的数据，则等待数据到达
        while self.pos as usize >= data.len() {
            // 检查下载是否已完成
            if self.download_completed.load(Ordering::Acquire) {
                // 下载已完成，没有更多数据了，返回 EOF
                return Ok(0);
            }

            if self.cancellation_token.is_cancelled() {
                // 播放已取消，跳出循环以防止阻塞
                return Ok(0);
            }
            // 等待更多数据或下载完成的通知
            data = self.condvar.wait(data).unwrap();
        }

        // 当前数据中可读的部分
        let available = &data[self.pos as usize..];

        // 截取可用数据
        let len = available.len().min(buf.len());
        buf[..len].copy_from_slice(&available[..len]);
        self.pos += len as u64;
        Ok(len)
    }
}

impl Seek for MVecU8Reader {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64> {
        let new_pos = match pos {
            SeekFrom::Start(p) => p,
            SeekFrom::Current(off) => (self.pos as i64 + off) as u64,
            SeekFrom::End(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "SeekFrom::End not supported",
                ));
            }
        };

        self.pos = new_pos;
        Ok(self.pos)
    }
}
