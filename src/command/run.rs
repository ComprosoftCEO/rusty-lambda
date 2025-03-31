use std::path::PathBuf;

use clap::Args;

#[derive(Args)]
pub struct RunArgs {
  /// Enter interactive mode after compiling files
  #[clap(short, long)]
  interactive: bool,

  /// List of files to run, in order
  files: Vec<PathBuf>,
}

impl RunArgs {
  pub fn execute(self) -> super::CommandResult {
    Ok(())
  }
}
