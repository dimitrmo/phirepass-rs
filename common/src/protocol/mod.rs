pub mod common;
#[cfg(not(target_arch = "wasm32"))]
pub mod node;
pub mod web;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Protocol {
    SSH = 0,
}

impl TryFrom<u8> for Protocol {
    type Error = ();

    fn try_from(value: u8) -> anyhow::Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::SSH),
            1_u8..=u8::MAX => todo!(),
        }
    }
}
