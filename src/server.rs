use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use failure::Fail;
use tokio::net::{TcpListener, TcpStream};

use crate::algorithm::Preference;
use crate::connection::Connection;
use crate::handler::Handler;
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
    timeout: Option<Duration>,
}

impl Default for ServerBuilder {
    fn default() -> Self {
        Self {
            version: None,
            preference: None,
            hostkeys: None,
            timeout: None,
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
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }
    pub async fn build<HF>(self, addr: SocketAddr, handler_factory: HF) -> io::Result<Server<HF>> {
        let socket = TcpListener::bind(addr).await?;
        Ok(Server {
            version: self.version.unwrap_or_else(|| "SSH-2.0-sssh".into()),
            addr,
            preference: self.preference.unwrap_or_default(),
            hostkeys: self
                .hostkeys
                .unwrap_or_else(|| HostKeys::new(vec![HostKey::gen_ssh_ed25519().unwrap()])),
            timeout: self.timeout,
            socket,
            handler_factory,
        })
    }
}

#[derive(Debug)]
pub struct Server<HF> {
    version: String,
    addr: SocketAddr,
    preference: Preference,
    hostkeys: HostKeys,
    socket: TcpListener,
    timeout: Option<Duration>,
    handler_factory: HF,
}

impl<HF, H> Server<HF>
where
    H: Handler,
    HF: Fn() -> H,
{
    pub async fn accept(&mut self) -> Result<Connection<TcpStream, H>, AcceptError> {
        let (socket, remote) = self.socket.accept().await?;
        Ok(Connection::establish(
            socket,
            self.version.clone(),
            remote,
            self.hostkeys.clone(),
            self.preference.clone(),
            self.timeout.clone(),
            (self.handler_factory)(),
        )
        .await?)
    }
}
