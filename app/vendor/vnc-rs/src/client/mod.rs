mod auth;
pub mod connection;
pub mod connector;
mod messages;

#[derive(Clone, Copy, Default)]
pub(crate) struct ProtocolOptions {
    pub levels: Option<(u8, u8)>,
    pub frame_boundaries: bool,
}
mod security;

pub use connection::VncClient;
pub use connector::VncConnector;

mod resize;
