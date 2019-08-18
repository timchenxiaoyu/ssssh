use std::error::Error as StdError;

use futures::future::{BoxFuture, FutureExt as _};

use crate::handle::{AuthHandle, ChannelHandle};

pub enum Auth {
    Accept,
    Reject,
}

pub trait Handler {
    type Error: Into<Box<dyn StdError + Send + Sync>>;

    fn auth_none(
        &mut self,
        _uesrname: &str,
        _auth_handle: &AuthHandle,
    ) -> BoxFuture<Result<Auth, Self::Error>> {
        async { Ok(Auth::Reject) }.boxed()
    }

    fn auth_publickey(
        &mut self,
        _username: &str,
        _publickey: &[u8],
        _handle: &AuthHandle,
    ) -> BoxFuture<Result<Auth, Self::Error>> {
        async { Ok(Auth::Reject) }.boxed()
    }

    fn auth_password(
        &mut self,
        _username: &str,
        _password: &[u8],
        _handle: &AuthHandle,
    ) -> BoxFuture<Result<Auth, Self::Error>> {
        async { Ok(Auth::Reject) }.boxed()
    }

    fn channel_open_session(
        &mut self,
        _handle: &ChannelHandle,
    ) -> BoxFuture<Result<(), Self::Error>> {
        async { Ok(()) }.boxed()
    }

    fn channel_pty_request(
        &mut self,
        _handle: &ChannelHandle,
    ) -> BoxFuture<Result<(), Self::Error>> {
        async { Ok(()) }.boxed()
    }

    fn channel_shell_request(
        &mut self,
        _handle: &ChannelHandle,
    ) -> BoxFuture<Result<(), Self::Error>> {
        async { Ok(()) }.boxed()
    }

    fn channel_exec_request(
        &mut self,
        _handle: &ChannelHandle,
    ) -> BoxFuture<Result<(), Self::Error>> {
        async { Ok(()) }.boxed()
    }

    fn channel_data(
        &mut self,
        _data: &[u8],
        _handle: &ChannelHandle,
    ) -> BoxFuture<Result<(), Self::Error>> {
        async { Ok(()) }.boxed()
    }

    fn channel_eof(&mut self, _handle: &ChannelHandle) -> BoxFuture<Result<(), Self::Error>> {
        async { Ok(()) }.boxed()
    }

    fn channel_close(&mut self, _handle: &ChannelHandle) -> BoxFuture<Result<(), Self::Error>> {
        async { Ok(()) }.boxed()
    }
}
