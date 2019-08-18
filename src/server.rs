use std::io;
use std::net::SocketAddr;

use failure::Fail;
use tokio::net::{TcpListener, TcpStream};

use crate::algorithm::Preference;
use crate::connection::Connection;
use crate::handler::{AuthHandler, ChannelHandler};
use crate::hostkey::{HostKey, HostKeys};
use crate::transport::version::VersionExchangeError;

#[derive(Debug, Fail)]
pub enum AcceptError {
    #[fail(display = "Invalid SSH identification string")]
    InvalidFormat,
    #[fail(display = "Io Error")]
    Io(#[fail(cause)] io::Error),
}

impl From<io::Error> for AcceptError {
    fn from(v: io::Error) -> Self {
        Self::Io(v)
    }
}

impl From<VersionExchangeError> for AcceptError {
    fn from(v: VersionExchangeError) -> Self {
        match v {
            VersionExchangeError::InvalidFormat => Self::InvalidFormat,
            VersionExchangeError::Io(e) => e.into(),
        }
    }
}

#[derive(Debug)]
#[allow(clippy::module_name_repetitions)]
pub struct ServerBuilder {
    version: Option<String>,
    preference: Option<Preference>,
    hostkeys: Option<HostKeys>,
}

impl Default for ServerBuilder {
    fn default() -> Self {
        Self {
            version: None,
            preference: None,
            hostkeys: None,
        }
    }
}

impl ServerBuilder {
    pub fn version(mut self, v: impl Into<String>) -> Self {
        self.version = Some(v.into());
        self
    }
    pub fn preference(mut self, v: Preference) -> Self {
        self.preference = Some(v);
        self
    }
    pub fn build<AHF, CHF>(
        self,
        addr: SocketAddr,
        auth_handler_factory: AHF,
        channel_handler_factory: CHF,
    ) -> Server<AHF, CHF> {
        Server {
            version: self.version.unwrap_or_else(|| "SSH-2.0-sssh".into()),
            addr,
            preference: self.preference.unwrap_or_default(),
            hostkeys: self
                .hostkeys
                .unwrap_or_else(|| HostKeys::new(vec![HostKey::gen_ssh_ed25519()])),
            socket: None,
            auth_handler_factory,
            channel_handler_factory,
        }
    }
}

#[derive(Debug)]
pub struct Server<AHF, CHF> {
    version: String,
    addr: SocketAddr,
    preference: Preference,
    hostkeys: HostKeys,
    socket: Option<TcpListener>,
    auth_handler_factory: AHF,
    channel_handler_factory: CHF,
}

impl<AHF, CHF, AH, CH> Server<AHF, CHF>
where
    AH: AuthHandler,
    CH: ChannelHandler,
    AHF: Fn() -> AH,
    CHF: Fn() -> CH,
{
    pub async fn accept(
        &mut self,
    ) -> Result<Connection<TcpStream, SocketAddr, AH, CH>, AcceptError> {
        if self.socket.is_none() {
            self.socket = Some(TcpListener::bind(&self.addr)?);
        }

        let socket = self.socket.as_mut().unwrap();

        let (socket, remote) = socket.accept().await?;
        Ok(Connection::establish(
            socket,
            self.version.clone(),
            remote,
            self.hostkeys.clone(),
            self.preference.clone(),
            (self.auth_handler_factory)(),
            (self.channel_handler_factory)(),
        )
        .await?)
    }
}
