use std::fs::File;
use std::io::BufWriter;

use anyhow::Result;

pub trait IntSeqEncoder {
    /// Creates an encoder
    fn new_encoder(universe: Option<usize>, num_elem: usize) -> Self
    where
        Self: Sized;

    /// Compresses a sorted slice of integers
    fn encode(&mut self, values: &[u64]) -> Result<()>;

    /// Returns the size of the encoded data (that would be written to disk)
    fn len(&self) -> usize;

    /// Writes to disk and returns number of bytes written (which can be just len(),
    /// or more if extra info is also required for decoding)
    fn write(&self, writer: &mut BufWriter<&mut File>) -> Result<usize>;
}
