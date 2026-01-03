use russh::client;
use russh::keys::PublicKey;

pub(crate) struct SSHClient {}

impl client::Handler for SSHClient {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &PublicKey,
    ) -> anyhow::Result<bool, Self::Error> {
        Ok(true)
    }
}
