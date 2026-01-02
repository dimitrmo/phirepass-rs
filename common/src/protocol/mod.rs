pub mod common;
pub mod conversions;
pub mod generated;
#[cfg(not(target_arch = "wasm32"))]
pub mod node;
pub mod sftp;
pub mod web;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Protocol {
    SSH = 0,
    SFTP = 1,
}

impl TryFrom<u8> for Protocol {
    type Error = ();

    fn try_from(value: u8) -> anyhow::Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::SSH),
            1 => Ok(Self::SFTP),
            2_u8..=u8::MAX => todo!(),
        }
    }
}
