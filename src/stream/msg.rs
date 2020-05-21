use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::BytesMut;
use futures::ready;
use futures::sink::Sink;
use futures::stream::Stream;
use log::debug;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncWrite, BufStream};

use super::bpp::{BppStream, DecodeError, EncodeError};
use crate::msg::{ContextualMsg, Msg};
use crate::pack::{Pack, Unpack, UnpackError};

#[derive(Debug, Error)]
pub enum RecvError {
    #[error(transparent)]
    DecodeError(#[from] DecodeError),

    #[error(transparent)]
    UnpackError(#[from] UnpackError),
}

#[derive(Debug, Error)]
pub enum SendError {
    #[error(transparent)]
    EncodeError(#[from] EncodeError),
}

#[derive(Debug)]
pub(crate) struct MsgStream<IO>
where
    IO: AsyncRead + AsyncWrite + Unpin,
{
    io: BppStream<IO>,
}

impl<IO> MsgStream<IO>
where
    IO: AsyncRead + AsyncWrite + Unpin,
{
    pub(crate) fn new(io: BufStream<IO>) -> Self {
        Self {
            io: BppStream::new(io),
        }
    }

    pub(crate) fn get_ref(&self) -> &BppStream<IO> {
        &self.io
    }

    pub(crate) fn get_mut(&mut self) -> &mut BppStream<IO> {
        &mut self.io
    }

    pub(crate) fn context<'a, M>(&'a mut self) -> ContextualMsgStream<'a, IO, M>
    where
        M: ContextualMsg + Unpin,
    {
        ContextualMsgStream {
            inner: self,
            _phantom: PhantomData,
        }
    }
}

impl<IO> Stream for MsgStream<IO>
where
    IO: AsyncRead + AsyncWrite + Unpin,
{
    type Item = Result<Msg, RecvError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let io = &mut self.get_mut().io;
        match ready!(Pin::new(io).poll_next(cx)?) {
            Some(ref mut buf) => {
                let msg = Unpack::unpack(buf)?;
                debug!("< {:?}", msg);
                Poll::Ready(Some(Ok(msg)))
            }
            None => Poll::Ready(None),
        }
    }
}

impl<IO> Sink<Msg> for MsgStream<IO>
where
    IO: AsyncRead + AsyncWrite + Unpin,
{
    type Error = SendError;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let io = &mut self.get_mut().io;
        ready!(Pin::new(io).poll_ready(cx))?;
        Poll::Ready(Ok(()))
    }

    fn start_send(self: Pin<&mut Self>, item: Msg) -> Result<(), Self::Error> {
        debug!("> {:?}", item);
        let mut buf = BytesMut::new();
        item.pack(&mut buf);
        let io = &mut self.get_mut().io;
        Pin::new(io).start_send(buf.freeze())?;
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let io = &mut self.get_mut().io;
        ready!(Pin::new(io).poll_flush(cx))?;
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let io = &mut self.get_mut().io;
        ready!(Pin::new(io).poll_close(cx))?;
        Poll::Ready(Ok(()))
    }
}

#[derive(Debug)]
pub(crate) struct ContextualMsgStream<'a, IO, M>
where
    IO: AsyncRead + AsyncWrite + Unpin,
    M: ContextualMsg + Unpin,
{
    inner: &'a mut MsgStream<IO>,
    _phantom: PhantomData<M>,
}

impl<'a, IO, M> Stream for ContextualMsgStream<'a, IO, M>
where
    IO: AsyncRead + AsyncWrite + Unpin,
    M: ContextualMsg + Unpin,
{
    type Item = Result<M, RecvError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let io = &mut self.get_mut().inner.io;
        match ready!(Pin::new(io).poll_next(cx)?) {
            Some(ref mut buf) => {
                let msg = Unpack::unpack(buf)?;
                debug!("< {:?}", msg);
                Poll::Ready(Some(Ok(msg)))
            }
            None => Poll::Ready(None),
        }
    }
}

impl<'a, IO, M> Sink<M> for ContextualMsgStream<'a, IO, M>
where
    IO: AsyncRead + AsyncWrite + Unpin,
    M: ContextualMsg + Unpin,
{
    type Error = SendError;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let io = &mut self.get_mut().inner.io;
        ready!(Pin::new(io).poll_ready(cx))?;
        Poll::Ready(Ok(()))
    }

    fn start_send(self: Pin<&mut Self>, item: M) -> Result<(), Self::Error> {
        debug!("> {:?}", item);
        let mut buf = BytesMut::new();
        item.pack(&mut buf);
        let io = &mut self.get_mut().inner.io;
        Pin::new(io).start_send(buf.freeze())?;
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let io = &mut self.get_mut().inner.io;
        ready!(Pin::new(io).poll_flush(cx))?;
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let io = &mut self.get_mut().inner.io;
        ready!(Pin::new(io).poll_close(cx))?;
        Poll::Ready(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_send() {
        fn assert<T: Send + Sync + 'static>() {}

        assert::<MsgStream<tokio::net::TcpStream>>();
        assert::<ContextualMsgStream<tokio::net::TcpStream, crate::msg::GexMsg>>();
        assert::<SendError>();
        assert::<RecvError>();
    }
}
