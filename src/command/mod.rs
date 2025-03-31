use std::error::Error;

mod encode;
pub(self) mod executor;
mod run;

pub use encode::EncodeArgs;
pub use run::RunArgs;

pub type CommandResult = std::result::Result<(), Box<dyn Error>>;
