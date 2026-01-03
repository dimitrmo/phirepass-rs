use std::collections::HashMap;
use std::sync::Arc;
use russh_sftp::client::fs::File;
use tokio::sync::Mutex;

pub const CHUNK_SIZE: usize = 64 * 1024; // 64KB chunks

pub type SFTPActiveUploads = Arc<Mutex<HashMap<(String, u32), File>>>;

pub mod client;
pub mod connection;
pub mod session;
pub mod actions;
