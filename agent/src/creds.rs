use directories::ProjectDirs;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub struct TokenStore {
    service: String,
    account: String,
    state_path: PathBuf,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
pub struct StoredState {
    pub node_id: Option<String>,
    pub token: Option<String>, // used only for fallback
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
    /// - token goes to keyring if possible
    /// - node_id always goes to state file
    /// - if keyring fails, token also goes to state file
    pub fn save(&self, node_id: Option<&str>, token: Option<&SecretString>) -> std::io::Result<()> {
        // Load existing state so we can update partially.
        let mut state = self.load_state()?.unwrap_or_default();

        if let Some(n) = node_id {
            state.node_id = Some(n.to_owned());
        }

        let mut keyring_ok = false;
        if let Some(tok) = token {
            if let Ok(entry) = keyring::Entry::new(&self.service, &self.account) {
                if entry.set_password(tok.expose_secret()).is_ok() {
                    keyring_ok = true;
                    // don’t keep token in file when keyring works
                    state.token = None;
                }
            }
            if !keyring_ok {
                // fallback: store token in file
                state.token = Some(tok.expose_secret().to_owned());
            }
        }

        self.save_state(&state)
    }

    pub fn load(&self) -> std::io::Result<(Option<String>, Option<SecretString>)> {
        let state = self.load_state()?.unwrap_or_default();

        // token: keyring first
        if let Ok(entry) = keyring::Entry::new(&self.service, &self.account) {
            if let Ok(s) = entry.get_password() {
                return Ok((state.node_id, Some(SecretString::from(s))));
            }
        }

        // fallback token from file
        let token = state.token.map(SecretString::from);
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

    fn load_state(&self) -> std::io::Result<Option<StoredState>> {
        match fs::read(&self.state_path) {
            Ok(bytes) => {
                let s: StoredState = serde_json::from_slice(&bytes).map_err(io_other)?;
                Ok(Some(s))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn save_state(&self, state: &StoredState) -> std::io::Result<()> {
        let bytes = serde_json::to_vec_pretty(state).map_err(io_other)?;
        atomic_write(&self.state_path, &bytes)
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "No parent dir"))?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(bytes)?;
    tmp.as_file().sync_all()?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tmp.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
    }

    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

fn io_other<E: std::fmt::Display>(e: E) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
}
