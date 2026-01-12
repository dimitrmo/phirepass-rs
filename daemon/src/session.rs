use crate::sftp::session::{SFTPCommand, SFTPSessionHandle};
use crate::ssh::session::{SSHCommand, SSHSessionHandle};
use dashmap::DashMap;
use log::info;
use std::fmt::{Display, Formatter};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::sync::mpsc::Sender;
use ulid::Ulid;

pub type TunnelSessions = Arc<DashMap<(Ulid, u32), SessionHandle>>;

static SESSION_ID: AtomicU32 = AtomicU32::new(1);

pub fn generate_session_id() -> u32 {
    SESSION_ID.fetch_add(1, Ordering::Relaxed)
}

#[derive(Debug)]
pub enum SessionHandle {
    Ssh(SSHSessionHandle),
    Sftp(SFTPSessionHandle),
}

impl Display for SessionHandle {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionHandle::Ssh(_) => write!(f, "SSHSessionHandle"),
            SessionHandle::Sftp(_) => write!(f, "SFTPSessionHandle"),
        }
    }
}

pub enum SessionCommand {
    Ssh(Sender<SSHCommand>),
    Sftp(Sender<SFTPCommand>),
}

impl SessionHandle {
    pub fn get_stdin(&self) -> SessionCommand {
        match self {
            SessionHandle::Ssh(ssh_handle) => SessionCommand::Ssh(ssh_handle.stdin.clone()),
            SessionHandle::Sftp(sftp_handle) => SessionCommand::Sftp(sftp_handle.stdin.clone()),
        }
    }

    pub async fn shutdown(self) {
        match self {
            SessionHandle::Ssh(ssh_handle) => {
                info!("shutting down ssh handle");
                ssh_handle.shutdown().await;
            }
            SessionHandle::Sftp(sftp_handle) => {
                info!("shutting down sftp handle");
                sftp_handle.shutdown().await;
            }
        }
    }
}
