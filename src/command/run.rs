use crate::expr::Allocator;
use clap::Args;
use crossterm::style::Stylize;
use rustyline::DefaultEditor;
use rustyline::config::Configurer;
use rustyline::error::ReadlineError;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use typed_arena::Arena;

use super::executor::Executor;

#[derive(Args)]
pub struct RunArgs {
  /// Enter interactive mode after compiling files
  #[clap(short, long)]
  interactive: bool,

  /// Print the individual reduction steps to stderr
  #[clap(short, long)]
  steps: bool,

  /// List of files to run, in order
  files: Vec<PathBuf>,
}

static ABORT_EXECUTION: AtomicBool = AtomicBool::new(false);

impl RunArgs {
  pub fn execute(self) -> super::CommandResult {
    let text_data = Arena::new();
    let executor = Executor::new();

    // Load the prelude
    {
      let prelude = text_data.alloc(crate::PRELUDE.to_string());
      executor.load_code(prelude.as_str(), Some("prelude"))?;
    }

    // Load and evaluate the code files
    for file in self.files.iter() {
      let file_data = text_data.alloc(fs::read_to_string(file)?);

      let to_evaluate = executor.load_code(file_data.as_str(), file.to_str())?;
      for expr in to_evaluate {
        let eval_allocator = Allocator::new();
        let result = executor.evaluate(&eval_allocator, expr, self.steps);
        println!("{result:#}");
      }
    }

    // Drop into interactive mode if required
    let should_enter_interactive_mode = self.interactive || self.files.is_empty();
    if !should_enter_interactive_mode {
      return Ok(());
    }

    if let Err(e) = ctrlc::set_handler(|| {
      ABORT_EXECUTION.store(true, Ordering::Relaxed);
    }) {
      eprint!("{}failed to set Ctrl+C handler: {}", "Warning: ".yellow(), e);
    }

    let mut editor = DefaultEditor::new()?;
    editor.set_auto_add_history(true);

    let mut should_exit = false;
    loop {
      let line = match editor.readline("> ") {
        Ok(line) => {
          should_exit = false;
          line
        },

        Err(ReadlineError::Eof) => return Ok(()),
        Err(ReadlineError::Interrupted) => {
          if should_exit {
            return Ok(());
          }

          should_exit = true;
          println!("(To exit, press Ctrl+C again or Ctrl+D or type :exit)");
          continue;
        },

        Err(e) => return Err(e.into()),
      };

      if line.trim().is_empty() {
        continue;
      }

      let line = text_data.alloc(line);
      let eval_allocator = Allocator::new();
      match executor.load_statement(&eval_allocator, line.as_str()) {
        Ok(None) => {},
        Ok(Some(expr)) => {
          ABORT_EXECUTION.store(false, Ordering::Relaxed);

          let result = executor.evaluate_with_abort(&eval_allocator, expr, self.steps, &ABORT_EXECUTION);
          match result {
            None => println!("Interrupted"),
            Some(result) => println!("{result:#}"),
          }
        },
        Err(e) => println!("{e}"),
      }
    }
  }
}
