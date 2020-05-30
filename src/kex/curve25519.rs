use futures::sink::SinkExt as _;
use ring::agreement::{agree_ephemeral, EphemeralPrivateKey, PublicKey, UnparsedPublicKey, X25519};
use ring::error::Unspecified;
use ring::rand::SystemRandom;
use tokio::stream::StreamExt as _;

use crate::msg::kex_ecdh_reply::KexEcdhReply;
use crate::pack::{Mpint, Pack};

use super::*;

#[derive(Debug)]
pub(crate) struct Curve25519Sha256 {}

#[async_trait]
impl KexTrait for Curve25519Sha256 {
    const NAME: &'static str = "curve25519-sha256";

    fn new() -> Self {
        Self {}
    }

    fn hasher() -> Hasher {
        Hasher::sha256()
    }

    async fn kex<IO>(
        &self,
        io: &mut MsgStream<IO>,
        env: Env<'_>,
    ) -> Result<(Bytes, Bytes), SshError>
    where
        IO: AsyncRead + AsyncWrite + Unpin + Send,
    {
        let mut hasher = Self::hasher();

        env.c_version.pack(&mut hasher);
        env.s_version.pack(&mut hasher);
        env.c_kexinit.pack(&mut hasher);
        env.s_kexinit.pack(&mut hasher);
        env.hostkey.publickey().pack(&mut hasher);

        let kex_ecdh_init = match io.next().await {
            Some(Ok(Msg::KexEcdhInit(msg))) => msg,
            Some(Ok(msg)) => return Err(SshError::KexUnexpectedMsg(format!("{:?}", msg))),
            Some(Err(e)) => return Err(e),
            None => return Err(SshError::KexUnexpectedEof),
        };

        let client_ephemeral_public_key = kex_ecdh_init.ephemeral_public_key();
        let client_ephemeral_public_key =
            UnparsedPublicKey::new(&X25519, client_ephemeral_public_key);
        Bytes::from(client_ephemeral_public_key.clone().bytes().to_vec()).pack(&mut hasher);

        let (server_ephemeral_private_key, server_ephemeral_public_key) = gen_keypair()?;
        Bytes::from(server_ephemeral_public_key.as_ref().to_vec()).pack(&mut hasher);

        let key = agree_ephemeral(
            server_ephemeral_private_key,
            &client_ephemeral_public_key,
            Unspecified,
            |mut e| Ok(e.to_bytes()),
        )
        .map_err(SshError::kex_error)?;
        Mpint::new(key.clone()).pack(&mut hasher);

        let hash = hasher.finish();

        let signature = env.hostkey.sign(&hash);

        let kex_ecdh_reply = KexEcdhReply::new(
            env.hostkey.publickey(),
            server_ephemeral_public_key.as_ref().to_bytes(),
            signature,
        );

        io.send(kex_ecdh_reply.into()).await?;

        Ok((hash, key))
    }
}

fn gen_keypair() -> Result<(EphemeralPrivateKey, PublicKey), SshError> {
    let rand = SystemRandom::new();
    let private = EphemeralPrivateKey::generate(&X25519, &rand).map_err(SshError::kex_error)?;
    let public = private.compute_public_key().map_err(SshError::kex_error)?;
    Ok((private, public))
}

impl From<Curve25519Sha256> for Kex {
    fn from(v: Curve25519Sha256) -> Self {
        Self::Curve25519Sha256(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_kex_send() {
        fn assert<T: Send>(t: T) -> T {
            t
        }

        let io = tokio::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/null")
            .await
            .unwrap();
        let io = tokio::io::BufStream::new(io);
        let mut io = crate::stream::msg::MsgStream::new(io);

        let hostkey = crate::hostkey::HostKey::gen("ssh-rsa").unwrap();

        let c_kexinit = crate::preference::PreferenceBuilder::default()
            .build()
            .unwrap()
            .to_kexinit();
        let s_kexinit = crate::preference::PreferenceBuilder::default()
            .build()
            .unwrap()
            .to_kexinit();

        let kex = assert(Curve25519Sha256::new());
        let env = Env {
            c_version: "",
            s_version: "",
            c_kexinit: &to_msg_bytes(&c_kexinit),
            s_kexinit: &to_msg_bytes(&s_kexinit),
            hostkey: &hostkey,
        };
        assert(kex.kex(&mut io, env));
    }
}
