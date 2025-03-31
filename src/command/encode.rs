use std::path::PathBuf;

use clap::Args;

/// Encode an expression to Binary Lambda Calculus
#[derive(Args)]
pub struct EncodeArgs {
  /// Name of the term to encode
  #[clap(short, long)]
  term: String,

  /// List of files to load
  #[clap(required = true)]
  files: Vec<PathBuf>,
}

impl EncodeArgs {
  pub fn execute(self) -> super::CommandResult {
    Ok(())
  }
}
