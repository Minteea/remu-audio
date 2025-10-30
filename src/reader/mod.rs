mod mutex_vec_bytes;
mod mutex_vec_u8;

pub use mutex_vec_bytes::{MVecBytesReader, MVecBytesWrapper};
pub use mutex_vec_u8::{MVecU8Reader, MVecU8Wrapper};

pub trait AppendableDataWrapper {
    fn append_data(&mut self, slice: &[u8]);
    fn complete(&mut self);
}
