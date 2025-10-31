use bytes::{Bytes, BytesMut};
use std::io::{Read, Result, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use tokio_util::sync::CancellationToken;

use super::AppendableDataWrapper;

#[derive(Debug, Clone)]
pub struct MVecBytesWrapper {
    data: Arc<Mutex<Vec<Bytes>>>,
    completed: Arc<AtomicBool>,
    chunk_size: usize,
    current_chunk: BytesMut,
}

impl MVecBytesWrapper {
    pub fn new(chunk_size: usize) -> Self {
        Self {
            data: Arc::new(Mutex::new(Vec::new())),
            completed: Arc::new(AtomicBool::new(false)),
            chunk_size,
            current_chunk: BytesMut::with_capacity(chunk_size),
        }
    }

    pub fn data(&self) -> Arc<Mutex<Vec<Bytes>>> {
        self.data.clone()
    }
    pub fn completed(&self) -> Arc<AtomicBool> {
        self.completed.clone()
    }
    pub fn chunk_size(&self) -> usize {
        self.chunk_size
    }
}

impl AppendableDataWrapper for MVecBytesWrapper {
    fn append_data(&mut self, slice: &[u8]) {
        if self.completed.load(Ordering::SeqCst) {
            return;
        }
        let current_chunk_len = self.current_chunk.len();

        // 情况1: current_chunk.len() + slice.len() <= chunk_size
        if current_chunk_len + slice.len() <= self.chunk_size {
            self.current_chunk.extend_from_slice(slice);
            // 如果恰好达到 chunk_size，冻结并推入 data
            if self.current_chunk.len() == self.chunk_size {
                self.data
                    .lock()
                    .unwrap()
                    .push(self.current_chunk.clone().freeze());

                // 重置 current_chunk
                self.current_chunk = BytesMut::with_capacity(self.chunk_size);
            }
        }
        // 情况2: current_chunk.len() + slice.len() > chunk_size
        else {
            let mut append_data: Vec<Bytes> = Vec::new();
            let mut offset = 0;

            // 如果 current_chunk 长度不为 0
            if current_chunk_len != 0 {
                let first_part_len = self.chunk_size - current_chunk_len;

                // 补齐 current_chunk 到 chunk_size，冻结并推入 append_data
                let first_part = &slice[..first_part_len];
                self.current_chunk.extend_from_slice(first_part);
                append_data.push(self.current_chunk.clone().freeze());

                offset += first_part_len;

                // 重置 current_chunk
                self.current_chunk = BytesMut::with_capacity(self.chunk_size);
            }
            // 按 chunk_size 分割 slice
            while offset + self.chunk_size <= slice.len() {
                append_data.push(Bytes::copy_from_slice(
                    &slice[offset..offset + self.chunk_size],
                ));
                offset += self.chunk_size;
            }

            // 处理最后一个不足 chunk_size 的部分
            if offset < slice.len() {
                let remaining = &slice[offset..];
                self.current_chunk.extend_from_slice(remaining);
            } else {
                // 如果刚好分割完，current_chunk 保持为空
                self.current_chunk = BytesMut::with_capacity(self.chunk_size);
            }

            // 将 append_data 中的所有完整块推入 data
            self.data.lock().unwrap().append(&mut append_data);
        }
    }
    fn complete(&mut self) {
        if self.current_chunk.len() > 0 {
            self.data
                .lock()
                .unwrap()
                .push(self.current_chunk.clone().freeze());
            self.current_chunk = BytesMut::new();
        }
        self.completed.store(true, Ordering::SeqCst);
    }
    fn set_capacity(&mut self, capacity: usize) {
        let mut data = self.data.lock().unwrap();
        let len = data.len();
        data.reserve_exact((capacity - len) / self.chunk_size + 1);
    }
}

pub struct MVecBytesReader {
    data: Arc<Mutex<Vec<Bytes>>>,
    chunk_size: usize,
    condvar: Arc<Condvar>,
    pos: u64,
    download_completed: Arc<AtomicBool>,
    cancellation_token: CancellationToken,
}

impl MVecBytesReader {
    pub fn new(wrapper: MVecBytesWrapper, condvar: Arc<Condvar>) -> Self {
        Self {
            data: wrapper.data(),
            condvar,
            chunk_size: wrapper.chunk_size(),
            pos: 0,
            download_completed: wrapper.completed(),
            cancellation_token: CancellationToken::new(),
        }
    }

    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation_token.clone()
    }
}

impl Read for MVecBytesReader {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let lock = &*self.data;
        let mut data = lock.lock().unwrap();

        // 如果需要读取的数据位置超出当前缓冲区的数据，则等待数据到达
        while self.pos as usize >= data.len() * self.chunk_size {
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

        // 找到当前位置所在的块
        let chunk_start_idx = self.pos as usize / self.chunk_size;
        let chunk_start_offset = self.pos as usize % self.chunk_size;

        let mut chunk_end_idx = (self.pos as usize + buf.len()) / self.chunk_size;
        let mut chunk_end_offset = (self.pos as usize + buf.len()) % self.chunk_size;

        if chunk_end_idx >= data.len() {
            chunk_end_idx = data.len();
            chunk_end_offset = 0;
        }

        // 获取起始块
        let start_chunk = data[chunk_start_idx].clone();

        // 获取中间块
        let middle_chunks: Option<Vec<Bytes>> = if chunk_end_idx - chunk_start_idx > 1 {
            Some(data[chunk_start_idx + 1..chunk_end_idx].to_vec())
        } else {
            None
        };

        // 获取结束块
        let end_chunk = if chunk_end_idx > chunk_start_idx && chunk_end_offset > 0 {
            Some(data[chunk_end_idx].clone())
        } else {
            None
        };
        drop(data);

        // 计算偏移量（总读取字节数）
        let mut offset: usize = 0;

        if chunk_start_idx == chunk_end_idx {
            // 只有一个块，直接读取
            let chunk = start_chunk;
            // 可读取长度
            let len = chunk_end_offset.min(chunk.len()) - chunk_start_offset;

            buf[..len].copy_from_slice(&chunk[chunk_start_offset..chunk_start_offset + len]);

            offset += len;
        } else {
            // 处理多个块的情况

            // 先处理起始块
            {
                // 首个分块可读取长度
                let len = start_chunk.len() - chunk_start_offset;
                buf[..len].copy_from_slice(&start_chunk[chunk_start_offset..]);
                offset += len;
            }

            // 处理中间块
            if let Some(middle_chunks) = middle_chunks {
                for chunk in middle_chunks {
                    // 可读取长度
                    let len = chunk.len();
                    buf[offset..offset + len].copy_from_slice(&chunk);
                    offset += len;
                }
            }

            // 处理结束块
            if let Some(end_chunk) = end_chunk {
                // 可读取长度
                let len = chunk_end_offset.min(end_chunk.len());
                buf[offset..offset + len].copy_from_slice(&end_chunk[..len]);
                offset += len;
            }
        }
        self.pos += offset as u64;
        Ok(offset)
    }
}

impl Seek for MVecBytesReader {
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
