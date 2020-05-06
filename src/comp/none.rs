//! `none` compression algorithm
use bytes::Buf as _;

use super::*;

/// `none` compression algorithm
#[derive(Debug)]
pub(crate) struct None {}

impl CompressionTrait for None {
    const NAME: &'static str = "none";

    fn new() -> Self {
        Self {}
    }

    fn compress(&self, mut target: &[u8]) -> Result<Bytes, CompressionError> {
        Ok(target.to_bytes())
    }

    fn decompress(&self, mut target: &[u8]) -> Result<Bytes, CompressionError> {
        Ok(target.to_bytes())
    }
}
