use std::error::Error;

mod decode;
mod encode;
mod executor;
mod run;

pub use decode::DecodeArgs;
pub use encode::EncodeArgs;
pub use run::RunArgs;

pub type CommandResult = std::result::Result<(), Box<dyn Error>>;
