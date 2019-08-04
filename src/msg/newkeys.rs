use std::io::Cursor;

use bytes::{Bytes, BytesMut, BufMut as _};

use super::{Message, MessageResult};
use super::super::MessageId;

#[derive(Debug)]
pub struct Newkeys;

impl Newkeys {
    pub fn from(_buf: Cursor<Bytes>) -> MessageResult<Self> {
        Ok(Newkeys)
    }
    pub fn put(&self, buf: &mut BytesMut) -> MessageResult<()> {
        buf.put_u8(MessageId::Newkeys as u8);
        Ok(())
    }
}

impl From<Newkeys> for Message {
    fn from(v: Newkeys) -> Message {
        Message::Newkeys(v)
    }
}
