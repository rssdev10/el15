//! SCPI / LXI Raw-Socket server emulating a RIGOL DL3000 series electronic
//! load on top of an EL15 device. Reference: `docs/DL3000_ProgrammingManual_EN.pdf`.
//!
//! Only the most common subset is implemented; unknown commands return an
//! SCPI-conformant `-113,"Undefined header"` reply for queries and silently
//! NOP for command-only headers (matching DL3000 behavior).

mod handlers;
pub mod server;
mod state;

pub use server::{ScpiServer, ScpiServerConfig, ScpiLogEntry, LogSink};
pub use state::SharedState;
