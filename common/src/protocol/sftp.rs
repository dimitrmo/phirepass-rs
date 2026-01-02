use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[repr(u8)]
pub enum SFTPListItemKind {
    File = 0,
    Folder = 1,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SFTPListItemAttributes {
    pub size: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SFTPListItem {
    pub name: String,
    pub path: String,
    pub kind: SFTPListItemKind,
    pub items: Vec<SFTPListItem>,
    pub attributes: SFTPListItemAttributes,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SFTPFileChunk {
    pub filename: String,
    pub chunk_index: u32,
    pub total_chunks: u32,
    pub total_size: u64,
    pub chunk_size: u32,
    pub data: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SFTPUploadChunk {
    pub filename: String,
    pub remote_path: String,
    pub chunk_index: u32,
    pub total_chunks: u32,
    pub total_size: u64,
    pub chunk_size: u32,
    pub data: Vec<u8>,
}
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SFTPDelete {
    pub path: String,
    pub filename: String,
}
