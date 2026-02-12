use anyhow::Context;
use directories::ProjectDirs;
use log::{debug, info, warn};
use phirepass_common::token::extract_creds;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use uuid::Uuid;

const KEYRING_SERVICE: &str = "phirepass-agent";

#[derive(Debug)]
pub struct TokenStore {
    service: String,
    account: String,
    state_path: PathBuf,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
pub struct StoredState {
    pub node_id: Uuid,
    pub token: String,
    #[serde(default)]
    pub server_host: String, // track which server these creds are for
}

impl TokenStore {
    pub fn new(org: &str, app: &str, service: &str, account: &str) -> std::io::Result<Self> {
        let proj = ProjectDirs::from("com", org, app)
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "No project dirs"))?;

        let dir = proj.data_local_dir();
        fs::create_dir_all(dir)?;

        Ok(Self {
            service: service.to_string(),
            account: account.to_string(),
            state_path: dir.join("state.json"),
        })
    }

    /// Save node_id and token.
    /// - token is stored in keyring first (primary)
    /// - if keyring fails, token is stored in the state file (fallback)
    pub fn save(&self, node_id: &str, tok: &SecretString) -> anyhow::Result<()> {
        debug!("saving credentials");

        let node_id = Uuid::parse_str(node_id).map_err(|e| {
            anyhow::anyhow!(
                "Invalid node_id format: '{}'. Expected valid UUID. Error: {}",
                node_id,
                e
            )
        })?;

        let state = StoredState {
            node_id,
            token: tok.expose_secret().to_owned(),
            server_host: self.service.clone(),
        };

        let payload = serde_json::to_string(&state).map_err(io_other)?;

        match keyring::Entry::new(KEYRING_SERVICE, &self.account) {
            Ok(entry) => match entry.set_password(&payload) {
                Ok(_) => {
                    debug!("credentials saved to keyring");
                    if let Err(e) = self.delete_state_file() {
                        warn!("could not delete state file after keyring save: {e}");
                    }
                    return Ok(());
                }
                Err(e) => {
                    warn!("could not save credentials to keyring ({}).", e);
                }
            },
            Err(_) => {
                debug!("Keyring backend unavailable.");
            }
        }

        info!("saving credentials to state file");
        self.save_state(&state)
    }

    pub fn load(&self) -> anyhow::Result<(Uuid, SecretString)> {
        debug!("loading credentials");

        let state = self.load_state()?.unwrap_or_default();

        if !state.server_host.is_empty() && state.server_host != self.service {
            anyhow::bail!(
                "Server mismatch: credentials are for '{}' but attempting to connect to '{}'. \
                 please login to the correct server or clear credentials.",
                state.server_host,
                self.service
            );
        }

        if state.node_id == Uuid::nil() {
            anyhow::bail!(
                "Stored node_id is nil (uninitialized). Token store needs to be re-initialized via login."
            );
        }

        if state.token.is_empty() {
            anyhow::bail!("stored token is empty. Please login again.");
        }

        let token = SecretString::from(state.token);

        // Validate token format using extract_creds
        extract_creds(token.expose_secret().to_string()).context(
            "token format validation failed. token may be corrupted. please login again.",
        )?;

        Ok((state.node_id, token))
    }

    pub fn delete(&self) -> std::io::Result<()> {
        if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, &self.account) {
            let _ = entry.delete_credential();
        }

        self.delete_state_file()
    }

    /// Public version of load_state for retrieving raw state without validation
    pub fn load_state_public(&self) -> anyhow::Result<Option<StoredState>> {
        self.load_state()
    }

    fn load_state(&self) -> anyhow::Result<Option<StoredState>> {
        if let Some(state) = self.load_state_from_keyring()? {
            return Ok(Some(state));
        }

        self.load_state_from_file()
    }

    fn save_state(&self, state: &StoredState) -> anyhow::Result<()> {
        let bytes = serde_json::to_vec_pretty(state).map_err(io_other)?;
        atomic_write(&self.state_path, &bytes)
    }

    fn load_state_from_keyring(&self) -> anyhow::Result<Option<StoredState>> {
        match keyring::Entry::new(KEYRING_SERVICE, &self.account) {
            Ok(entry) => match entry.get_password() {
                Ok(payload) => match serde_json::from_str::<StoredState>(&payload) {
                    Ok(state) => {
                        debug!("credentials retrieved from keyring");
                        Ok(Some(state))
                    }
                    Err(e) => {
                        warn!(
                            "Failed to deserialize keyring credentials: {}. Falling back to state file.",
                            e
                        );
                        Ok(None)
                    }
                },
                Err(e) => {
                    debug!("keyring read failed, falling back to state file: {e}");
                    Ok(None)
                }
            },
            Err(e) => {
                debug!("Keyring backend unavailable: {e}");
                Ok(None)
            }
        }
    }

    fn load_state_from_file(&self) -> anyhow::Result<Option<StoredState>> {
        match fs::read(&self.state_path) {
            Ok(bytes) => match serde_json::from_slice::<StoredState>(&bytes) {
                Ok(s) => Ok(Some(s)),
                Err(e) => {
                    warn!(
                        "Failed to deserialize state from {:?}: {}. Error details: line {}, column {}. \
                         This may indicate state file corruption. Resetting state.",
                        self.state_path,
                        e,
                        e.line(),
                        e.column()
                    );
                    Ok(None)
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn delete_state_file(&self) -> std::io::Result<()> {
        match fs::remove_file(&self.state_path) {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "No parent dir"))?;

    // Ensure directory permissions are secure (owner only: 0o700)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if dir.exists() {
            match fs::set_permissions(dir, fs::Permissions::from_mode(0o700)) {
                Ok(_) => {
                    debug!("Set directory permissions to 0o700 (owner only)");
                }
                Err(e) => {
                    warn!("Could not set directory permissions to 0o700: {}", e);
                }
            }
        }
    }

    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(bytes)?;
    tmp.as_file().sync_all()?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tmp.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
        debug!("Set state file permissions to 0o600 (owner read/write only)");
    }

    tmp.persist(path).map_err(|e| e.error)?;
    info!("State file persisted to {:?}", path);
    Ok(())
}

fn io_other<E: std::fmt::Display>(e: E) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
}
