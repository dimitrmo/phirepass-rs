//! Conversions between protobuf and Rust enum types

use super::generated::phirepass;
use super::web::WebFrameData;
#[cfg(not(target_arch = "wasm32"))]
use super::node::NodeFrameData;
use super::sftp::{SFTPDelete, SFTPFileChunk, SFTPListItem, SFTPUploadChunk};
use super::common::FrameError;
use anyhow::anyhow;

// ============================================================================
// WebFrameData conversions
// ============================================================================

impl TryFrom<WebFrameData> for phirepass::frame::frame::Data {
    type Error = anyhow::Error;

    fn try_from(data: WebFrameData) -> Result<Self, Self::Error> {
        let web_data = match data {
            WebFrameData::Heartbeat => {
                phirepass::web::web_frame_data::Message::Heartbeat(phirepass::web::Heartbeat {})
            }
            WebFrameData::OpenTunnel {
                protocol,
                node_id,
                msg_id,
                username,
                password,
            } => phirepass::web::web_frame_data::Message::OpenTunnel(phirepass::web::OpenTunnel {
                protocol: protocol as u32,
                node_id,
                msg_id,
                username,
                password,
            }),
            WebFrameData::TunnelOpened {
                protocol,
                sid,
                msg_id,
            } => phirepass::web::web_frame_data::Message::TunnelOpened(phirepass::web::TunnelOpened {
                protocol: protocol as u32,
                sid,
                msg_id,
            }),
            WebFrameData::TunnelData {
                protocol,
                node_id,
                sid,
                data,
            } => phirepass::web::web_frame_data::Message::TunnelData(phirepass::web::TunnelData {
                protocol: protocol as u32,
                node_id,
                sid,
                data,
            }),
            WebFrameData::TunnelClosed {
                protocol,
                sid,
                msg_id,
            } => phirepass::web::web_frame_data::Message::TunnelClosed(phirepass::web::TunnelClosed {
                protocol: protocol as u32,
                sid,
                msg_id,
            }),
            WebFrameData::SSHWindowResize {
                node_id,
                sid,
                cols,
                rows,
            } => phirepass::web::web_frame_data::Message::SshWindowResize(phirepass::web::SshWindowResize {
                node_id,
                sid,
                cols,
                rows,
            }),
            WebFrameData::SFTPList {
                node_id,
                path,
                sid,
                msg_id,
            } => phirepass::web::web_frame_data::Message::SftpList(phirepass::web::SftpList {
                node_id,
                path,
                sid,
                msg_id,
            }),
            WebFrameData::SFTPListItems {
                path,
                sid,
                dir,
                msg_id,
            } => {
                phirepass::web::web_frame_data::Message::SftpListItems(phirepass::web::SftpListItems {
                    path,
                    sid,
                    dir: Some(dir.into()),
                    msg_id,
                })
            }
            WebFrameData::SFTPDownload {
                node_id,
                path,
                filename,
                sid,
                msg_id,
            } => phirepass::web::web_frame_data::Message::SftpDownload(phirepass::web::SftpDownload {
                node_id,
                path,
                filename,
                sid,
                msg_id,
            }),
            WebFrameData::SFTPUpload {
                node_id,
                path,
                sid,
                msg_id,
                chunk,
            } => phirepass::web::web_frame_data::Message::SftpUpload(phirepass::web::SftpUpload {
                node_id,
                path,
                sid,
                msg_id,
                chunk: Some(chunk.into()),
            }),
            WebFrameData::SFTPDelete {
                node_id,
                sid,
                msg_id,
                data,
            } => phirepass::web::web_frame_data::Message::SftpDelete(phirepass::web::SftpDelete {
                node_id,
                sid,
                msg_id,
                data: Some(data.into()),
            }),
            WebFrameData::SFTPFileChunk { sid, msg_id, chunk } => {
                phirepass::web::web_frame_data::Message::SftpFileChunk(phirepass::web::SftpFileChunk {
                    sid,
                    msg_id,
                    chunk: Some(chunk.into()),
                })
            }
            WebFrameData::Error {
                kind,
                message,
                msg_id,
            } => phirepass::web::web_frame_data::Message::Error(phirepass::web::Error {
                kind: kind as i32,
                message,
                msg_id,
            }),
        };

        Ok(phirepass::frame::frame::Data::Web(phirepass::web::WebFrameData {
            message: Some(web_data),
        }))
    }
}

impl TryFrom<phirepass::frame::frame::Data> for WebFrameData {
    type Error = anyhow::Error;

    fn try_from(data: phirepass::frame::frame::Data) -> Result<Self, <Self as TryFrom<phirepass::frame::frame::Data>>::Error> {
        match data {
            phirepass::frame::frame::Data::Web(web_frame) => {
                let message = web_frame
                    .message
                    .ok_or_else(|| anyhow!("empty web frame message"))?;

                match message {
                    phirepass::web::web_frame_data::Message::Heartbeat(_) => Ok(WebFrameData::Heartbeat),
                    phirepass::web::web_frame_data::Message::OpenTunnel(msg) => {
                        Ok(WebFrameData::OpenTunnel {
                            protocol: msg.protocol as u8,
                            node_id: msg.node_id,
                            msg_id: msg.msg_id,
                            username: msg.username,
                            password: msg.password,
                        })
                    }
                    phirepass::web::web_frame_data::Message::TunnelOpened(msg) => {
                        Ok(WebFrameData::TunnelOpened {
                            protocol: msg.protocol as u8,
                            sid: msg.sid,
                            msg_id: msg.msg_id,
                        })
                    }
                    phirepass::web::web_frame_data::Message::TunnelData(msg) => {
                        Ok(WebFrameData::TunnelData {
                            protocol: msg.protocol as u8,
                            node_id: msg.node_id,
                            sid: msg.sid,
                            data: msg.data,
                        })
                    }
                    phirepass::web::web_frame_data::Message::TunnelClosed(msg) => {
                        Ok(WebFrameData::TunnelClosed {
                            protocol: msg.protocol as u8,
                            sid: msg.sid,
                            msg_id: msg.msg_id,
                        })
                    }
                    phirepass::web::web_frame_data::Message::SshWindowResize(msg) => {
                        Ok(WebFrameData::SSHWindowResize {
                            node_id: msg.node_id,
                            sid: msg.sid,
                            cols: msg.cols,
                            rows: msg.rows,
                        })
                    }
                    phirepass::web::web_frame_data::Message::SftpList(msg) => Ok(WebFrameData::SFTPList {
                        node_id: msg.node_id,
                        path: msg.path,
                        sid: msg.sid,
                        msg_id: msg.msg_id,
                    }),
                    phirepass::web::web_frame_data::Message::SftpListItems(msg) => {
                        Ok(WebFrameData::SFTPListItems {
                            path: msg.path,
                            sid: msg.sid,
                            dir: msg
                                .dir
                                .ok_or_else(|| anyhow!("missing SFTP list item"))?
                                .try_into()?,
                            msg_id: msg.msg_id,
                        })
                    }
                    phirepass::web::web_frame_data::Message::SftpDownload(msg) => {
                        Ok(WebFrameData::SFTPDownload {
                            node_id: msg.node_id,
                            path: msg.path,
                            filename: msg.filename,
                            sid: msg.sid,
                            msg_id: msg.msg_id,
                        })
                    }
                    phirepass::web::web_frame_data::Message::SftpUpload(msg) => {
                        Ok(WebFrameData::SFTPUpload {
                            node_id: msg.node_id,
                            path: msg.path,
                            sid: msg.sid,
                            msg_id: msg.msg_id,
                            chunk: msg
                                .chunk
                                .ok_or_else(|| anyhow!("missing upload chunk"))?
                                .try_into()?,
                        })
                    }
                    phirepass::web::web_frame_data::Message::SftpDelete(msg) => {
                        Ok(WebFrameData::SFTPDelete {
                            node_id: msg.node_id,
                            sid: msg.sid,
                            msg_id: msg.msg_id,
                            data: msg
                                .data
                                .ok_or_else(|| anyhow!("missing delete data"))?
                                .try_into()?,
                        })
                    }
                    phirepass::web::web_frame_data::Message::SftpFileChunk(msg) => {
                        Ok(WebFrameData::SFTPFileChunk {
                            sid: msg.sid,
                            msg_id: msg.msg_id,
                            chunk: msg
                                .chunk
                                .ok_or_else(|| anyhow!("missing file chunk"))?
                                .try_into()?,
                        })
                    }
                    phirepass::web::web_frame_data::Message::Error(msg) => Ok(WebFrameData::Error {
                        kind: FrameError::from(msg.kind as u8),
                        message: msg.message,
                        msg_id: msg.msg_id,
                    }),
                }
            }
            phirepass::frame::frame::Data::Node(_) => {
                Err(anyhow!("expected web frame data, got node frame data"))
            }
        }
    }
}

// ============================================================================
// SFTP type conversions - Simple wrapper approach
// ============================================================================

impl From<SFTPListItem> for phirepass::sftp::SftpListItem {
    fn from(item: SFTPListItem) -> Self {
        // Serialize to JSON as bytes for now
        let data = serde_json::to_vec(&item).unwrap_or_default();
        Self { data }
    }
}

impl TryFrom<phirepass::sftp::SftpListItem> for SFTPListItem {
    type Error = anyhow::Error;

    fn try_from(item: phirepass::sftp::SftpListItem) -> Result<Self, Self::Error> {
        serde_json::from_slice(&item.data)
            .map_err(|e| anyhow!("failed to deserialize SFTP list item: {}", e))
    }
}

impl From<SFTPUploadChunk> for phirepass::sftp::SftpUploadChunk {
    fn from(chunk: SFTPUploadChunk) -> Self {
        let data = serde_json::to_vec(&chunk).unwrap_or_default();
        Self { data }
    }
}

impl TryFrom<phirepass::sftp::SftpUploadChunk> for SFTPUploadChunk {
    type Error = anyhow::Error;

    fn try_from(chunk: phirepass::sftp::SftpUploadChunk) -> Result<Self, Self::Error> {
        serde_json::from_slice(&chunk.data)
            .map_err(|e| anyhow!("failed to deserialize SFTP upload chunk: {}", e))
    }
}

impl From<SFTPFileChunk> for phirepass::sftp::SftpFileChunk {
    fn from(chunk: SFTPFileChunk) -> Self {
        let data = serde_json::to_vec(&chunk).unwrap_or_default();
        Self { data }
    }
}

impl TryFrom<phirepass::sftp::SftpFileChunk> for SFTPFileChunk {
    type Error = anyhow::Error;

    fn try_from(chunk: phirepass::sftp::SftpFileChunk) -> Result<Self, Self::Error> {
        serde_json::from_slice(&chunk.data)
            .map_err(|e| anyhow!("failed to deserialize SFTP file chunk: {}", e))
    }
}

impl From<SFTPDelete> for phirepass::sftp::SftpDelete {
    fn from(delete: SFTPDelete) -> Self {
        let data = serde_json::to_vec(&delete).unwrap_or_default();
        Self { data }
    }
}

impl TryFrom<phirepass::sftp::SftpDelete> for SFTPDelete {
    type Error = anyhow::Error;

    fn try_from(delete: phirepass::sftp::SftpDelete) -> Result<Self, Self::Error> {
        serde_json::from_slice(&delete.data)
            .map_err(|e| anyhow!("failed to deserialize SFTP delete: {}", e))
    }
}
// ============================================================================
// NodeFrameData conversions (server <-> daemon)
// ============================================================================

#[cfg(not(target_arch = "wasm32"))]
impl TryFrom<NodeFrameData> for phirepass::frame::frame::Data {
    type Error = anyhow::Error;

    fn try_from(data: NodeFrameData) -> Result<Self, Self::Error> {
        let node_data = match data {
            NodeFrameData::Heartbeat { stats } => {
                phirepass::node::node_frame_data::Message::Heartbeat(phirepass::node::Heartbeat {
                    stats: Some(phirepass::node::Stats {
                        host_cpu: stats.host_cpu,
                        host_mem_used_bytes: stats.host_mem_used_bytes,
                        host_mem_total_bytes: stats.host_mem_total_bytes,
                        network_bytes_sent: 0,
                        network_bytes_received: 0,
                        uptime_seconds: stats.host_uptime_secs,
                    }),
                })
            }
            NodeFrameData::Auth { token } => {
                phirepass::node::node_frame_data::Message::Auth(phirepass::node::Auth { token })
            }
            NodeFrameData::AuthResponse {
                node_id,
                success,
                version,
            } => phirepass::node::node_frame_data::Message::AuthResponse(
                phirepass::node::AuthResponse {
                    node_id,
                    success,
                    version,
                },
            ),
            NodeFrameData::OpenTunnel {
                protocol,
                cid,
                username,
                password,
                msg_id,
            } => phirepass::node::node_frame_data::Message::OpenTunnel(
                phirepass::node::OpenTunnel {
                    protocol: protocol as u32,
                    cid,
                    username,
                    password,
                    msg_id,
                },
            ),
            NodeFrameData::TunnelOpened {
                protocol,
                cid,
                sid,
                msg_id,
            } => phirepass::node::node_frame_data::Message::TunnelOpened(
                phirepass::node::TunnelOpened {
                    protocol: protocol as u32,
                    cid,
                    sid,
                    msg_id,
                },
            ),
            NodeFrameData::TunnelData {
                protocol,
                cid,
                sid,
                data,
            } => phirepass::node::node_frame_data::Message::TunnelData(
                phirepass::node::TunnelData {
                    protocol: protocol as u32,
                    cid,
                    sid,
                    data,
                },
            ),
            NodeFrameData::TunnelClosed {
                protocol,
                cid,
                sid,
                msg_id,
            } => phirepass::node::node_frame_data::Message::TunnelClosed(
                phirepass::node::TunnelClosed {
                    protocol: protocol as u32,
                    cid,
                    sid,
                    msg_id,
                },
            ),
            NodeFrameData::SSHWindowResize {
                cid,
                sid,
                cols,
                rows,
            } => phirepass::node::node_frame_data::Message::SshWindowResize(
                phirepass::node::SshWindowResize {
                    cid,
                    sid,
                    cols,
                    rows,
                },
            ),
            NodeFrameData::SFTPList {
                cid,
                path,
                sid,
                msg_id,
            } => phirepass::node::node_frame_data::Message::SftpList(phirepass::node::SftpList {
                cid,
                path,
                sid,
                msg_id,
            }),
            NodeFrameData::SFTPDownload {
                cid,
                path,
                filename,
                sid,
                msg_id,
            } => phirepass::node::node_frame_data::Message::SftpDownload(
                phirepass::node::SftpDownload {
                    cid,
                    path,
                    filename,
                    sid,
                    msg_id,
                },
            ),
            NodeFrameData::SFTPUpload {
                cid,
                path,
                sid,
                msg_id,
                chunk,
            } => phirepass::node::node_frame_data::Message::SftpUpload(
                phirepass::node::SftpUpload {
                    cid,
                    path,
                    sid,
                    msg_id,
                    chunk: Some(chunk.into()),
                },
            ),
            NodeFrameData::SFTPDelete {
                cid,
                sid,
                msg_id,
                data,
            } => phirepass::node::node_frame_data::Message::SftpDelete(
                phirepass::node::SftpDelete {
                    cid,
                    sid,
                    msg_id,
                    data: Some(data.into()),
                },
            ),
            NodeFrameData::Ping { sent_at } => {
                phirepass::node::node_frame_data::Message::Ping(phirepass::node::Ping { sent_at })
            }
            NodeFrameData::Pong { sent_at } => {
                phirepass::node::node_frame_data::Message::Pong(phirepass::node::Pong { sent_at })
            }
            NodeFrameData::WebFrame { frame, sid } => {
                let web_proto_data: phirepass::frame::frame::Data = frame.try_into()?;
                if let phirepass::frame::frame::Data::Web(web_data) = web_proto_data {
                    phirepass::node::node_frame_data::Message::WebFrame(
                        phirepass::node::WebFrame {
                            frame: Some(web_data),
                            sid,
                        },
                    )
                } else {
                    return Err(anyhow!("expected web frame data"));
                }
            }
            NodeFrameData::ConnectionDisconnect { cid } => {
                phirepass::node::node_frame_data::Message::ConnectionDisconnect(
                    phirepass::node::ConnectionDisconnect { cid },
                )
            }
        };

        Ok(phirepass::frame::frame::Data::Node(
            phirepass::node::NodeFrameData {
                message: Some(node_data),
            },
        ))
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl TryFrom<phirepass::frame::frame::Data> for NodeFrameData {
    type Error = anyhow::Error;

    fn try_from(data: phirepass::frame::frame::Data) -> Result<Self, <Self as TryFrom<phirepass::frame::frame::Data>>::Error> {
        use crate::stats::Stats;

        match data {
            phirepass::frame::frame::Data::Node(node_frame) => {
                let message = node_frame
                    .message
                    .ok_or_else(|| anyhow!("empty node frame message"))?;

                match message {
                    phirepass::node::node_frame_data::Message::Heartbeat(msg) => {
                        let stats = msg.stats.ok_or_else(|| anyhow!("missing stats"))?;
                        Ok(NodeFrameData::Heartbeat {
                            stats: Stats {
                                proc_id: String::new(),
                                proc_threads: 0,
                                proc_cpu: 0.0,
                                proc_mem_bytes: 0,
                                proc_uptime_secs: 0,
                                host_name: String::new(),
                                host_ip: String::new(),
                                host_mac: String::new(),
                                host_cpu: stats.host_cpu,
                                host_mem_used_bytes: stats.host_mem_used_bytes,
                                host_mem_total_bytes: stats.host_mem_total_bytes,
                                host_uptime_secs: stats.uptime_seconds,
                                host_load_average: [0.0, 0.0, 0.0],
                                host_os_info: String::new(),
                                host_connections: 0,
                                host_processes: 0,
                            },
                        })
                    }
                    phirepass::node::node_frame_data::Message::Auth(msg) => {
                        Ok(NodeFrameData::Auth { token: msg.token })
                    }
                    phirepass::node::node_frame_data::Message::AuthResponse(msg) => {
                        Ok(NodeFrameData::AuthResponse {
                            node_id: msg.node_id,
                            success: msg.success,
                            version: msg.version,
                        })
                    }
                    phirepass::node::node_frame_data::Message::OpenTunnel(msg) => {
                        Ok(NodeFrameData::OpenTunnel {
                            protocol: msg.protocol as u8,
                            cid: msg.cid,
                            username: msg.username,
                            password: msg.password,
                            msg_id: msg.msg_id,
                        })
                    }
                    phirepass::node::node_frame_data::Message::TunnelOpened(msg) => {
                        Ok(NodeFrameData::TunnelOpened {
                            protocol: msg.protocol as u8,
                            cid: msg.cid,
                            sid: msg.sid,
                            msg_id: msg.msg_id,
                        })
                    }
                    phirepass::node::node_frame_data::Message::TunnelData(msg) => {
                        Ok(NodeFrameData::TunnelData {
                            protocol: msg.protocol as u8,
                            cid: msg.cid,
                            sid: msg.sid,
                            data: msg.data,
                        })
                    }
                    phirepass::node::node_frame_data::Message::TunnelClosed(msg) => {
                        Ok(NodeFrameData::TunnelClosed {
                            protocol: msg.protocol as u8,
                            cid: msg.cid,
                            sid: msg.sid,
                            msg_id: msg.msg_id,
                        })
                    }
                    phirepass::node::node_frame_data::Message::SshWindowResize(msg) => {
                        Ok(NodeFrameData::SSHWindowResize {
                            cid: msg.cid,
                            sid: msg.sid,
                            cols: msg.cols,
                            rows: msg.rows,
                        })
                    }
                    phirepass::node::node_frame_data::Message::SftpList(msg) => {
                        Ok(NodeFrameData::SFTPList {
                            cid: msg.cid,
                            path: msg.path,
                            sid: msg.sid,
                            msg_id: msg.msg_id,
                        })
                    }
                    phirepass::node::node_frame_data::Message::SftpDownload(msg) => {
                        Ok(NodeFrameData::SFTPDownload {
                            cid: msg.cid,
                            path: msg.path,
                            filename: msg.filename,
                            sid: msg.sid,
                            msg_id: msg.msg_id,
                        })
                    }
                    phirepass::node::node_frame_data::Message::SftpUpload(msg) => {
                        Ok(NodeFrameData::SFTPUpload {
                            cid: msg.cid,
                            path: msg.path,
                            sid: msg.sid,
                            msg_id: msg.msg_id,
                            chunk: msg
                                .chunk
                                .ok_or_else(|| anyhow!("missing upload chunk"))?
                                .try_into()?,
                        })
                    }
                    phirepass::node::node_frame_data::Message::SftpDelete(msg) => {
                        Ok(NodeFrameData::SFTPDelete {
                            cid: msg.cid,
                            sid: msg.sid,
                            msg_id: msg.msg_id,
                            data: msg
                                .data
                                .ok_or_else(|| anyhow!("missing delete data"))?
                                .try_into()?,
                        })
                    }
                    phirepass::node::node_frame_data::Message::Ping(msg) => {
                        Ok(NodeFrameData::Ping { sent_at: msg.sent_at })
                    }
                    phirepass::node::node_frame_data::Message::Pong(msg) => {
                        Ok(NodeFrameData::Pong { sent_at: msg.sent_at })
                    }
                    phirepass::node::node_frame_data::Message::WebFrame(msg) => {
                        let web_data = msg
                            .frame
                            .ok_or_else(|| anyhow!("missing web frame data"))?;
                        let web_frame = WebFrameData::try_from(
                            phirepass::frame::frame::Data::Web(web_data),
                        )?;
                        Ok(NodeFrameData::WebFrame {
                            frame: web_frame,
                            sid: msg.sid,
                        })
                    }
                    phirepass::node::node_frame_data::Message::ConnectionDisconnect(msg) => {
                        Ok(NodeFrameData::ConnectionDisconnect { cid: msg.cid })
                    }
                }
            }
            _ => Err(anyhow!("expected node frame data")),
        }
    }
}
