use std::collections::HashMap;
use std::error::Error as StdError;
use std::fmt::Display;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use failure::Fail;
use futures::channel::mpsc;
use futures::stream::{SplitSink, SplitStream};
use futures::{SinkExt as _, /*StreamExt as _*/};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::stream::{Stream, StreamExt as _};

use crate::algorithm::{Algorithm, Preference};
use crate::handle::{AuthHandle, ChannelHandle, GlobalHandle};
use crate::handler::{Auth, Handler, PasswordAuth, PasswordChangeAuth, Unsupported};
use crate::hostkey::HostKeys;
use crate::kex::{kex, KexEnv};
use crate::msg::{self, Message, MessageError, MessageResult};
use crate::transport::version::{Version, VersionExchangeResult};
use crate::transport::{ChangeKeyError, State, Transport};

#[derive(Debug, Fail)]
#[fail(display = "Running error {}", _0)]
pub struct Error(#[fail(cause)] ConnectionError);

#[derive(Debug, Fail)]
#[allow(clippy::module_name_repetitions)]
pub(crate) enum ConnectionError {
    #[fail(display = "Kex Error {:?}", _0)]
    KexError(Box<dyn Send + Sync + std::fmt::Debug>), // TODO StdError
    #[fail(display = "Message Error {}", _0)]
    MessageError(MessageError),
    #[fail(display = "Auth Error {}", _0)]
    AuthError(Box<dyn StdError + Send + Sync>),
    #[fail(display = "Channel Error {}", _0)]
    ChannelError(Box<dyn StdError + Send + Sync>),
    #[fail(display = "Unknown {}", _0)]
    Unknown(String),
    #[fail(display = "Unknown Channel Id {}", _0)]
    UnknownChannelId(u32),
    #[fail(display = "Send error occurred")]
    SendError(#[fail(cause)] mpsc::SendError),
    #[fail(display = "Unable to acquire shared state lock")]
    UnabledToSharedStateLock,
    #[fail(display = "ChangeKeyError")]
    ChangeKeyError(#[fail(cause)] ChangeKeyError),
    #[fail(display = "Timeout")]
    Timeout(#[fail(cause)] tokio::time::Elapsed),
    //#[fail(display = "Io Error {}", _0)]
    //Io(io::Error),
}

impl From<MessageError> for ConnectionError {
    fn from(v: MessageError) -> Self {
        Self::MessageError(v)
    }
}

impl From<mpsc::SendError> for ConnectionError {
    fn from(v: mpsc::SendError) -> Self {
        Self::SendError(v)
    }
}

impl From<ChangeKeyError> for ConnectionError {
    fn from(v: ChangeKeyError) -> Self {
        Self::ChangeKeyError(v)
    }
}

impl From<tokio::time::Elapsed> for ConnectionError {
    fn from(v: tokio::time::Elapsed) -> Self {
        Self::Timeout(v)
    }
}

#[allow(clippy::module_name_repetitions)]
pub(crate) type ConnectionResult<T> = Result<T, ConnectionError>;

#[derive(Debug)]
pub struct Connection<IO, H, R>
where
    IO: AsyncRead + AsyncWrite + Unpin,
{
    version: Version,
    rx: Option<SplitStream<Transport<IO>>>,
    message_receive: Option<mpsc::Receiver<Message>>,
    tx: SplitSink<Transport<IO>, Message>,
    state: Arc<Mutex<State>>,
    remote: R,
    hostkeys: HostKeys,
    preference: Preference,
    message_send: mpsc::Sender<Message>,
    handler: H,
    global_handle: GlobalHandle,
    auth_handle: Option<AuthHandle>,
    channel_handles: HashMap<u32, ChannelHandle>,
    timeout: Option<Duration>,
}

impl<IO, H, R> Connection<IO, H, R>
where
    IO: AsyncRead + AsyncWrite + Unpin,
    H: Handler,
    R: Display,
{
    pub async fn establish(
        mut socket: IO,
        server_version: impl Into<Bytes>,
        remote: R,
        hostkeys: HostKeys,
        preference: Preference,
        timeout: Option<Duration>,
        handler: H,
    ) -> VersionExchangeResult<Self> {
        log::debug!("Connecting.. {}", remote);

        let (version, rbuf) = Version::exchange(&mut socket, server_version).await?;
        let (message_send, message_receive) = mpsc::channel(0xFFFF); // TODO

        let state = Arc::new(Mutex::new(State::new()));
        let io = Transport::new(socket, rbuf, state.clone());
        let (tx, rx) = futures::stream::StreamExt::split(io);
        let global_handle = GlobalHandle::new(message_send.clone());

        let rx = Some(rx);
        let message_receive = Some(message_receive);
        log::debug!("Established.. {} version: {:?}", remote, version);

        Ok(Self {
            version,
            rx,
            tx,
            state,
            remote,
            hostkeys,
            preference,
            handler,
            message_receive,
            message_send,
            global_handle,
            auth_handle: None,
            channel_handles: HashMap::new(),
            timeout,
        })
    }

    pub fn handler(&self) -> &H {
        &self.handler
    }

    pub fn remote(&self) -> &R {
        &self.remote
    }

    pub async fn run(mut self) -> Result<(), Error> {
        log::debug!("running {}", self.remote);

        if let Err(e) = self.run0().await {
            log::error!("Error occurred {:?}", e);
            self.send_immediately(msg::Disconnect::new(2, "unexpected", ""))
                .await
                .ok(); // TODO
            Err(Error(e))
        } else {
            log::debug!("done {}", self.remote);
            Ok(())
        }
    }

    async fn send(&mut self, msg: impl Into<Message>) -> ConnectionResult<()> {
        let msg = msg.into();
        log::trace!("into sending queue {:?}", msg);
        self.message_send.send(msg).await?;
        Ok(())
    }

    async fn send_immediately(&mut self, msg: impl Into<Message>) -> MessageResult<()> {
        let msg = msg.into();
        self.tx.send(msg).await?;
        Ok(())
    }

    async fn run0(&mut self) -> ConnectionResult<()> {
        use msg::Message::*;

        let mut rx = self.rx.take().unwrap().timeout(self.timeout.unwrap());
        let mut message_receive = self.message_receive.take().unwrap();
        loop {
            tokio::select! {
                Some(m) = rx.next() => {
                    log::trace!("processing {:?}", m);
                    match m?? {
                    (_seq, Kexinit(item)) => self.on_kexinit(*item, &mut (&mut rx).map(|e| e.unwrap())).await?,
                    (_seq, ServiceRequest(item)) => self.on_service_request(item).await?,
                    (_seq, UserauthRequest(item)) => self.on_userauth_request(item).await?,
                    (_seq, ChannelOpen(item)) => self.on_channel_open(item).await?,
                    (_seq, ChannelRequest(item)) => self.on_channel_request(item).await?,
                    (_seq, ChannelData(item)) => self.on_channel_data(item).await?,
                    (_seq, ChannelEof(item)) => self.on_channel_eof(item).await?,
                    (_seq, ChannelClose(item)) => self.on_channel_close(item).await?,
                    (_seq, ChannelWindowAdjust(item)) => {
                        self.on_channel_window_adjust(item).await?
                    }
                    (_seq, GlobalRequest(item)) => self.on_global_request(item).await?,
                    (_seq, Ignore(..)) => {}
                    (_seq, Unimplemented(item)) => self.on_unimplemented(item).await?,
                    (_seq, Disconnect(item)) => {
                        self.on_disconnect(item).await?;
                        break;
                    }
                    (seq, x) => {
                        log::debug!("{:?}", x);
                        self.send(msg::Unimplemented::new(seq)).await?;
                    }
                    }
                }
                Some(m) = message_receive.next() => {
                    log::trace!("processing {:?}", m);
                    self.send_immediately(m).await?;
                }
                else => break
            }
        }
        Ok(())
    }

    async fn on_kexinit(&mut self, client_kexinit: msg::Kexinit, rx: &mut (impl Stream<Item=Result<(u32, msg::Message), msg::MessageError>> + Unpin)) -> ConnectionResult<()> {
        log::debug!("Begin kex {} {:?}", self.remote, client_kexinit);

        let server_kexinit = self.preference.to_kexinit();
        self.send_immediately(server_kexinit.clone())
            .await
            .map_err(|e| ConnectionError::KexError(Box::new(e)))?;

        let algorithm = Algorithm::negotiate(&client_kexinit, &self.preference)
            .map_err(|e| ConnectionError::KexError(Box::new(e)))?;
        log::debug!("Negotiate {} {:?}", self.remote, algorithm);

        let hostkey = self
            .hostkeys
            .lookup(&algorithm.server_host_key_algorithm())
            .ok_or_else(|| ConnectionError::KexError(Box::new("no match hostkye")))?; // TODO

        let mut env = KexEnv::new(
            &mut self.tx,
            rx,
            &self.version,
            &client_kexinit,
            &server_kexinit,
            hostkey,
        );
        let (h, k) = kex(algorithm.kex_algorithm(), &mut env)
            .await
            .map_err(|e| ConnectionError::KexError(Box::new(e)))?
            .split();
        log::debug!("Kex done {}", self.remote);

        match rx.next().await {
            Some(Ok((_, Message::Newkeys(..)))) => {
                self.send_immediately(msg::Newkeys).await?;
            }
            Some(e) => return Err(ConnectionError::KexError(Box::new(e))),
            None => return Err(ConnectionError::KexError(Box::new("No packet recieved"))), // TODO
        }

        let mut state = self
            .state
            .lock()
            .map_err(|_| ConnectionError::UnabledToSharedStateLock)?;
        state.change_key(&h, &k, &algorithm)?;

        Ok(())
    }

    async fn on_service_request(&mut self, msg: msg::ServiceRequest) -> ConnectionResult<()> {
        match msg.name() {
            "ssh-userauth" => self.send(msg::ServiceAccept::new(msg.name())).await?,
            _ => return Err(ConnectionError::Unknown(msg.name().into())), // TODO
        }
        Ok(())
    }

    async fn on_userauth_request(&mut self, msg: msg::UserauthRequest) -> ConnectionResult<()> {
        use msg::UserauthRequestMethod as M;

        if msg.service_name() != "ssh-connection" {
            return Err(ConnectionError::Unknown(msg.service_name().into())); // TODO
        };

        if self.auth_handle.is_none() {
            self.auth_handle = Some(self.global_handle.new_auth_handle())
        };
        let handle = self.auth_handle.as_ref().expect("never occurred");

        match &msg.method() {
            M::None => {
                let result = self
                    .handler
                    .auth_none(msg.user_name(), &handle)
                    .await
                    .map_err(|e| ConnectionError::AuthError(e.into()))?;
                match result {
                    Auth::Accept => self.send(msg::UserauthSuccess).await?,
                    Auth::Reject => {
                        self.send(msg::UserauthFailure::new(
                            vec!["publickey", "password"],
                            false,
                        ))
                        .await?
                    }
                };
            }

            M::Publickey(item) => {
                if let Some(_signature) = item.signature() {
                    // TODO CHECK
                    self.send(msg::UserauthSuccess).await?
                } else {
                    let result = self
                        .handler
                        .auth_publickey(msg.user_name(), item.blob(), &handle)
                        .await
                        .map_err(|e| ConnectionError::AuthError(e.into()))?;
                    match result {
                        Auth::Accept => {
                            self.send(msg::UserauthPkOk::new(
                                item.algorithm(),
                                item.blob().clone(),
                            ))
                            .await?
                        }
                        Auth::Reject => {
                            self.send(msg::UserauthFailure::new(
                                vec!["publickey", "password"],
                                false,
                            ))
                            .await?
                        }
                    };
                }
            }

            M::Password(item) => {
                if let Some(newpassword) = item.newpassword() {
                    let result = self
                        .handler
                        .auth_password_change(
                            msg.user_name(),
                            item.password(),
                            newpassword,
                            &handle,
                        )
                        .await
                        .map_err(|e| ConnectionError::AuthError(e.into()))?;
                    match result {
                        PasswordChangeAuth::Accept => self.send(msg::UserauthSuccess).await?,
                        PasswordChangeAuth::ChangePasswdreq(msg) => {
                            self.send(msg::UserauthPasswdChangereq::new(msg, ""))
                                .await?
                        }
                        PasswordChangeAuth::Partial => {
                            self.send(msg::UserauthFailure::new(
                                vec!["publickey", "password"],
                                true,
                            ))
                            .await?
                        }
                        PasswordChangeAuth::Reject => {
                            self.send(msg::UserauthFailure::new(
                                vec!["publickey", "password"],
                                false,
                            ))
                            .await?
                        }
                    };
                } else {
                    let result = self
                        .handler
                        .auth_password(msg.user_name(), item.password().as_bytes(), &handle)
                        .await
                        .map_err(|e| ConnectionError::AuthError(e.into()))?;
                    match result {
                        PasswordAuth::Accept => self.send(msg::UserauthSuccess).await?,
                        PasswordAuth::ChangePasswdreq(msg) => {
                            self.send(msg::UserauthPasswdChangereq::new(msg, ""))
                                .await?
                        }
                        PasswordAuth::Reject => {
                            self.send(msg::UserauthFailure::new(
                                vec!["publickey", "password"],
                                false,
                            ))
                            .await?
                        }
                    };
                }
            }
            M::Hostbased(..) | _ => {
                dbg!(&msg);
                self.send(msg::UserauthFailure::new(
                    vec!["publickey", "password"],
                    false,
                ))
                .await?;
            }
        };
        Ok(())
    }

    async fn on_channel_open(&mut self, msg: msg::ChannelOpen) -> ConnectionResult<()> {
        use msg::ChannelOpenChannelType::*;

        let result: msg::Message = match msg.channel_type() {
            Session => {
                let channel_handle = self.global_handle.new_channel_handle(msg.sender_channel());
                self.channel_handles
                    .insert(channel_handle.channel_id(), channel_handle);
                let channel_handle = self
                    .channel_handles
                    .get(&msg.sender_channel())
                    .expect("never occurred");

                match self.handler.channel_open_session(channel_handle).await {
                    Ok(..) => msg::ChannelOpenConfirmation::new(
                        msg.sender_channel(),
                        msg.sender_channel(),
                        msg.initial_window_size(),
                        msg.maximum_packet_size(),
                    )
                    .into(),
                    Err(e) => {
                        self.channel_handles.remove(&msg.sender_channel());
                        log::debug!("Failed to open channel {}", e);
                        msg::ChannelOpenFailure::new(
                            msg.sender_channel(),
                            msg::ChannelOpenFailureReasonCode::ConnectFailed,
                            "Failed to open channel",
                            "",
                        )
                        .into()
                    }
                }
            }
            t => {
                log::warn!("Unknown channel type {:?}", t);
                msg::ChannelOpenFailure::new(
                    msg.sender_channel(),
                    msg::ChannelOpenFailureReasonCode::UnknownChannelType,
                    "Unknown Channel Type",
                    "",
                )
                .into()
            }
        };
        self.send(result).await?;
        Ok(())
    }

    async fn on_channel_request(&mut self, msg: msg::ChannelRequest) -> ConnectionResult<()> {
        use msg::ChannelRequestType::*;

        let channel_id = &msg.recipient_channel();
        let handle = self
            .channel_handles
            .get(&channel_id)
            .ok_or_else(|| ConnectionError::UnknownChannelId(*channel_id))?;

        let result = match msg.request_type() {
            PtyReq(item) => {
                self.handler
                    .channel_pty_request(item.clone().into(), handle)
                    .await
            }
            Shell => self.handler.channel_shell_request(handle).await,
            Exec(path) => self.handler.channel_exec_request(path, handle).await,
            x => {
                log::warn!("Unknown channel request {:?}", x);
                Err(Unsupported.into())
            }
        };
        match result {
            Ok(()) => {
                if msg.want_reply() {
                    self.send(msg::ChannelSuccess::new(msg.recipient_channel()))
                        .await?;
                }
            }
            Err(e) => {
                log::debug!("Channel request Failed {}", e);
                if msg.want_reply() {
                    self.send(msg::ChannelFailure::new(msg.recipient_channel()))
                        .await?;
                }
            }
        };

        Ok(())
    }

    async fn on_channel_data(&mut self, msg: msg::ChannelData) -> ConnectionResult<()> {
        let channel_id = msg.recipient_channel();
        let handle = self
            .channel_handles
            .get(&channel_id)
            .ok_or_else(|| ConnectionError::UnknownChannelId(channel_id))?;

        let r = self.handler.channel_data(&msg.data(), handle).await;
        r.map_err(|e| ConnectionError::ChannelError(e.into()))?;
        Ok(())
    }

    async fn on_channel_eof(&mut self, msg: msg::ChannelEof) -> ConnectionResult<()> {
        let channel_id = msg.recipient_channel();
        let handle = self
            .channel_handles
            .get(&msg.recipient_channel())
            .ok_or_else(|| ConnectionError::UnknownChannelId(channel_id))?;

        let r = self.handler.channel_eof(handle).await;
        r.map_err(|e| ConnectionError::ChannelError(e.into()))?;
        Ok(())
    }

    async fn on_channel_close(&mut self, msg: msg::ChannelClose) -> ConnectionResult<()> {
        let channel_id = msg.recipient_channel();
        let handle = self
            .channel_handles
            .remove(&msg.recipient_channel())
            .ok_or_else(|| ConnectionError::UnknownChannelId(channel_id))?;

        let r = self.handler.channel_close(&handle).await;
        r.map_err(|e| ConnectionError::ChannelError(e.into()))?; // TODO
        Ok(())
    }

    async fn on_global_request(&mut self, msg: msg::GlobalRequest) -> ConnectionResult<()> {
        // TODO
        log::warn!("Not implemented {:?}", msg);
        self.send(msg::RequestFailure).await?;
        Ok(())
    }

    async fn on_channel_window_adjust(
        &mut self,
        msg: msg::ChannelWindowAdjust,
    ) -> ConnectionResult<()> {
        // TODO
        self.send(msg).await?;
        Ok(())
    }

    async fn on_disconnect(&mut self, msg: msg::Disconnect) -> ConnectionResult<()> {
        // TODO
        log::warn!("Not implemented {:?}", msg);
        Ok(())
    }

    async fn on_unimplemented(&mut self, msg: msg::Unimplemented) -> ConnectionResult<()> {
        // TODO
        log::warn!("Not implemented {:?}", msg);
        Ok(())
    }
}
