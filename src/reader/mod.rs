mod mutex_vec_bytes;
mod mutex_vec_u8;

pub use mutex_vec_bytes::{MVecBytesReader, MVecBytesWrapper};
pub use mutex_vec_u8::{MVecU8Reader, MVecU8Wrapper};

pub trait AppendableDataWrapper {
    /// 添加数据
    fn append_data(&mut self, slice: &[u8]);
    /// 完成数据添加
    fn complete(&mut self);
    /// 设置容量
    fn set_capacity(&mut self, capacity: usize);
}
