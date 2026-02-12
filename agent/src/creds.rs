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

#[derive(Debug)]
pub struct TokenStore {
    service: String,
    account: String,
    state_path: PathBuf,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
pub struct StoredState {
    pub node_id: Uuid,
    pub token: String, // used only for fallback
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
    /// - token is stored in the state file (primary)
    /// - token also goes to keyring if possible (backup/faster access)
    /// - node_id always goes to state file
    pub fn save(&self, node_id: &str, tok: &SecretString) -> anyhow::Result<()> {
        debug!("saving credentials to state file");

        // Load existing state so we can update partially.
        let mut state = self.load_state()?.unwrap_or_default();

        state.node_id = Uuid::parse_str(node_id).map_err(|e| {
            anyhow::anyhow!(
                "Invalid node_id format: '{}'. Expected valid UUID. Error: {}",
                node_id,
                e
            )
        })?;

        // Store server_host for validation on load
        state.server_host = self.service.clone();

        // ALWAYS store token in state file - this is the primary source of truth
        state.token = tok.expose_secret().to_owned();
        info!("token stored in state file");

        // Use a fixed keyring service name so it doesn't vary with server_host
        let keyring_service = "phirepass-agent";

        match keyring::Entry::new(keyring_service, &self.account) {
            Ok(entry) => match entry.set_password(tok.expose_secret()) {
                Ok(_) => {
                    debug!("token saved to keyring");
                }
                Err(e) => {
                    warn!(
                        "could not save token to keyring ({}). Token is safely stored in state file.",
                        e
                    );
                }
            },
            Err(_) => {
                debug!("Keyring backend unavailable. Token stored in state file.");
            }
        }

        self.save_state(&state)
    }

    pub fn load(&self) -> anyhow::Result<(Uuid, SecretString)> {
        debug!("loading credentials from state file");

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

        // Prefer state file as primary source (more reliable than keyring)
        let token = if !state.token.is_empty() {
            debug!("using token from state file");
            SecretString::from(state.token)
        } else {
            match keyring::Entry::new("phirepass-agent", &self.account) {
                Ok(entry) => match entry.get_password() {
                    Ok(s) => {
                        debug!("Token retrieved from keyring");
                        SecretString::from(s)
                    }
                    Err(e) => {
                        anyhow::bail!(
                            "no token found in state file or keyring. Please login first. \
                                 (Keyring error: {})",
                            e
                        );
                    }
                },
                Err(e) => {
                    anyhow::bail!(
                        "no token found in state file or keyring. Please login first. \
                         (Keyring unavailable: {})",
                        e
                    );
                }
            }
        };

        // Validate token format using extract_creds
        extract_creds(token.expose_secret().to_string()).context(
            "token format validation failed. token may be corrupted. please login again.",
        )?;

        Ok((state.node_id, token))
    }

    pub fn delete(&self) -> std::io::Result<()> {
        if let Ok(entry) = keyring::Entry::new(&self.service, &self.account) {
            let _ = entry.delete_credential();
        }

        match fs::remove_file(&self.state_path) {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Public version of load_state for retrieving raw state without validation
    pub fn load_state_public(&self) -> std::io::Result<Option<StoredState>> {
        self.load_state()
    }

    fn load_state(&self) -> std::io::Result<Option<StoredState>> {
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
            Err(e) => Err(e),
        }
    }

    fn save_state(&self, state: &StoredState) -> anyhow::Result<()> {
        let bytes = serde_json::to_vec_pretty(state).map_err(io_other)?;
        atomic_write(&self.state_path, &bytes)
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
