use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[repr(u8)]
pub enum SFTPListItemKind {
    File = 0,
    Folder = 1,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SFTPListItemAttributes {
    pub size: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SFTPListItem {
    pub name: String,
    pub path: String,
    pub kind: SFTPListItemKind,
    pub items: Vec<SFTPListItem>,
    pub attributes: SFTPListItemAttributes,
}
