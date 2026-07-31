//! Platform-independent domain rules for PeerSpan.

mod coordinates;
mod model;
mod pairing;
mod store;

pub use coordinates::{ContentRect, map_normalized_pointer, normalize_pointer};
pub use model::*;
pub use pairing::{PairingCode, PairingCodeError};
pub use store::{CoreError, PeerSpanCore};
