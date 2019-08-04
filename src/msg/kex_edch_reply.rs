use std::io::Cursor;

use bytes::{Bytes, BytesMut, BufMut as _};

use super::{Message, MessageResult, MessageId};
use crate::sshbuf::{SshBuf as _, SshBufMut as _};

#[derive(Debug)]
pub struct KexEdchReply {
    public_host_key: Vec<u8>,
    ephemeral_public_key: Vec<u8>,
    signature: Vec<u8>,
}

impl KexEdchReply {
    pub fn new(public_host_key: &[u8], ephemeral_public_key: &[u8], signature: &[u8]) -> Self {
        let public_host_key = Vec::from(public_host_key);
        let ephemeral_public_key = Vec::from(ephemeral_public_key);
        let signature = Vec::from(signature);
        Self { public_host_key, ephemeral_public_key, signature}
    }
    pub fn from(mut buf: Cursor<Bytes>) -> MessageResult<Self> {
        let public_host_key = buf.get_binary_string()?;
        let ephemeral_public_key = buf.get_binary_string()?;
        let signature = buf.get_binary_string()?;
        Ok(Self { public_host_key, ephemeral_public_key, signature })
    }
    pub fn put(&self, buf: &mut BytesMut) -> MessageResult<()> {
        buf.put_u8(MessageId::KexEcdhReply as u8);
        buf.put_binary_string(&{
            let mut buf = BytesMut::with_capacity(1024 * 8);
            buf.put_string("ssh-ed25519")?; // xxxx
            buf.put_binary_string(&self.public_host_key)?;
            buf

        })?;
        buf.put_binary_string(&self.ephemeral_public_key)?;
        buf.put_binary_string(&{
            let mut b = BytesMut::with_capacity(1024 * 8);
            b.put_string("ssh-ed25519")?; // xxx
            b.put_binary_string(&self.signature)?;
            b
        })?;
        Ok(())
    }
}

impl From<KexEdchReply> for Message {
    fn from(v: KexEdchReply) -> Message {
        Message::KexEdchReply(v)
    }
}
