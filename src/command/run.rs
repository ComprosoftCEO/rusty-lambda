use crate::expr::Allocator;
use clap::Args;
use rustyline::DefaultEditor;
use std::fs;
use std::path::PathBuf;
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

    let mut last_line: &String = text_data.alloc(String::new());
    let mut editor = DefaultEditor::new()?;
    loop {
      let line = editor.readline("> ")?;
      if line.trim().is_empty() {
        continue;
      }

      let line = text_data.alloc(line);
      let eval_allocator = Allocator::new();
      match executor.load_statement(&eval_allocator, line.as_str()) {
        Ok(None) => {},
        Ok(Some(expr)) => {
          let result = executor.evaluate(&eval_allocator, expr, self.steps);
          println!("{result:#}");
        },
        Err(e) => println!("{e}"),
      }

      // Save line into REPL history
      if line != last_line {
        editor.add_history_entry(line.as_str()).ok();
        last_line = line;
      }
    }
  }
}
