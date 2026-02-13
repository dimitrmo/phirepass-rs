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
    pub fn new(org: &str, app: &str, service: &str) -> std::io::Result<Self> {
        let proj = ProjectDirs::from("com", org, app)
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "No project dirs"))?;

        let dir = proj.data_local_dir();
        fs::create_dir_all(dir)?;

        debug!(
            "creating token store in {}",
            dir.join("stats.json").display()
        );

        if let Ok(exists) = fs::exists(dir) {
            debug!("directory {dir:?} exists: {exists}");
        }

        if let Ok(exists) = fs::exists(dir.join("state.json")) {
            debug!("directory {:?} exists: {}", dir.join("state.json"), exists);
        }

        Ok(Self {
            service: service.to_string(),
            state_path: dir.join("state.json"),
        })
    }

    /// Save node_id and token to the state file.
    pub fn save(&self, node_id: &str, tok: &SecretString) -> anyhow::Result<()> {
        debug!("saving credentials");

        let node_id = Uuid::parse_str(node_id).map_err(|e| {
            anyhow::anyhow!(
                "Invalid node_id format: '{}'. Expected valid UUID. Error: {}",
                node_id,
                e
            )
        })?;

        debug!("node id parsed {node_id}");

        let state = StoredState {
            node_id,
            token: tok.expose_secret().to_owned(),
            server_host: self.service.clone(),
        };

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
        self.delete_state_file()
    }

    /// Public version of load_state for retrieving raw state without validation
    pub fn load_state_public(&self) -> anyhow::Result<Option<StoredState>> {
        self.load_state()
    }

    fn load_state(&self) -> anyhow::Result<Option<StoredState>> {
        self.load_state_from_file()
    }

    fn save_state(&self, state: &StoredState) -> anyhow::Result<()> {
        let bytes = serde_json::to_vec_pretty(state).map_err(io_other)?;
        atomic_write(&self.state_path, &bytes)
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
