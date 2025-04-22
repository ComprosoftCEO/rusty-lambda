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

    Repl::new(&text_data, &executor, self.steps).run()
  }
}

struct Repl<'text, 'assign>
where
  'text: 'assign,
{
  text_data: &'text Arena<String>,
  executor: &'assign Executor<'assign>,
  show_steps: bool,
  abort: &'static AtomicBool,
}

enum RunLineAction {
  Continue,
  Exit,
}

impl<'text, 'assign> Repl<'text, 'assign>
where
  'text: 'assign,
{
  pub fn new(text_data: &'text Arena<String>, executor: &'assign Executor<'assign>, show_steps: bool) -> Self {
    static ABORT_EXECUTION: AtomicBool = AtomicBool::new(false);

    Self {
      text_data,
      executor,
      show_steps,
      abort: &ABORT_EXECUTION,
    }
  }

  pub fn run(mut self) -> super::CommandResult {
    // Initialize the Ctrl+C handler
    if let Err(e) = ctrlc::set_handler(|| {
      self.abort.store(true, Ordering::Relaxed);
    }) {
      println!("{}: failed to set Ctrl+C handler", "Warning".yellow());
      println!("{e}\n");
    }

    // Set up REPL editor
    let mut editor = DefaultEditor::new()?;
    editor.set_auto_add_history(true);

    // We only want to exit if Ctrl+C pressed twice in a row
    let mut ctrl_c_should_exit = false;

    println!("Welcome to Rusty Lambda, a lambda calculus interpreter");
    println!("Type \":help\" for more information");
    loop {
      let line = match editor.readline("> ") {
        Ok(line) => {
          ctrl_c_should_exit = false;
          line
        },

        Err(ReadlineError::Eof) => return Ok(()),
        Err(ReadlineError::Interrupted) => {
          if ctrl_c_should_exit {
            return Ok(());
          }

          ctrl_c_should_exit = true;
          println!("(To exit, press Ctrl+C again or Ctrl+D or type :exit)");
          continue;
        },

        Err(e) => return Err(e.into()),
      };

      if line.trim().is_empty() {
        continue; // Skip empty lines
      }

      match self.run_line(line) {
        RunLineAction::Continue => continue,
        RunLineAction::Exit => return Ok(()),
      }
    }
  }

  fn run_line(&mut self, line: String) -> RunLineAction {
    // Check for built-in commands
    let mut command_parts = line.split_whitespace();
    match command_parts.next() {
      Some(":e" | ":ex" | ":exi" | ":exit") => return RunLineAction::Exit,
      Some(":q" | ":qu" | ":qui" | ":quit") => return RunLineAction::Exit,
      Some(":h" | ":he" | ":hel" | ":help") => self.print_help(),
      Some(":s" | ":st" | ":ste" | ":step" | ":steps") => self.set_steps(&line, command_parts.collect()),
      Some(":a" | ":al" | ":all") => self.print_all_globals(),
      Some(prefix @ (":p" | ":pr" | ":pri" | ":prin" | ":print")) => {
        self.print_expression(strip_prefix(&line, prefix).to_string())
      },
      Some(prefix @ (":l" | ":lo" | ":loa" | ":load")) => self.load_file(strip_prefix(&line, prefix)),

      // Not a built-in command, so run the line as code
      None | Some(_) => self.run_line_as_code(line),
    }

    RunLineAction::Continue
  }

  fn print_help(&self) {
    static ALL_COMMANDS: &[(&str, &str)] = &[
      (":all", "Print all named variables"),
      (":exit", "Exit the REPL"),
      (":help", "Print this help message"),
      (":load <file>", "Load and run a code file"),
      (":print <expr>", "Print an expression without evaluating it"),
      (":quit", "Alias for :exit"),
      (":steps on", "Print reduction steps to stderr"),
      (":steps off", "Don't print reduction steps"),
    ];

    let max_name_length = ALL_COMMANDS.iter().map(|(name, _)| (*name).len()).max().unwrap_or(1);
    for (name, desc) in ALL_COMMANDS {
      println!(
        "{}  {desc}",
        format!("{name: <width$}", width = max_name_length).white().bold(),
      );
    }
    println!("\nPress Ctrl+C to abort current expression, Ctrl+D to exit the REPL");
  }

  fn set_steps(&mut self, line: &str, args: Vec<&str>) {
    match args.first().cloned() {
      None => {
        if self.show_steps {
          println!("Reduction steps are {}", "on".green());
        } else {
          println!("Reduction steps are {}", "off".red());
        }
      },

      Some("on" | "1" | "true") if args.len() == 1 => self.show_steps = true,

      Some("off" | "0" | "false") if args.len() == 1 => self.show_steps = false,

      Some(_) => {
        println!(
          "Expecting either '{}' or '{}', given '{line}'",
          ":steps on".white().bold(),
          ":steps off".white().bold(),
        )
      },
    }
  }

  fn print_all_globals(&self) {
    let all_globals = self.executor.all_globals().borrow();

    let max_name_length = all_globals.keys().map(|name| (*name).len()).max().unwrap_or(1);
    for (name, value) in all_globals.iter() {
      println!(
        "{} = {value:#}",
        format!("{name: <width$}", width = max_name_length).white().bold(),
      );
    }
  }

  fn print_expression(&self, expr: String) {
    let line = self.text_data.alloc(expr);
    let eval_allocator = Allocator::new();
    match self.executor.load_expression(&eval_allocator, line.as_str()) {
      Ok(expr) => println!("{expr:#}"),
      Err(e) => println!("{e}"),
    }
  }

  fn load_file(&self, filename: &str) {
    let result = (|| -> super::CommandResult {
      println!("Loading file: {}", filename.white());

      let file_data = self.text_data.alloc(fs::read_to_string(filename)?);
      let to_evaluate = self.executor.load_code(file_data.as_str(), Some(filename))?;

      println!("Running code...");
      for expr in to_evaluate {
        let eval_allocator = Allocator::new();
        self.abort.store(false, Ordering::Relaxed);

        let result = self
          .executor
          .evaluate_with_abort(&eval_allocator, expr, self.show_steps, self.abort);

        match result {
          None => println!("Interrupted"),
          Some(result) => println!("{result:#}"),
        }
      }

      Ok(())
    })();

    if let Err(e) = result {
      println!("{} {e}", "Error:".red());
    }
  }

  fn run_line_as_code(&self, line: String) {
    let line = self.text_data.alloc(line);
    let eval_allocator = Allocator::new();

    match self.executor.load_statement(&eval_allocator, line.as_str()) {
      Ok(None) => {},
      Ok(Some(expr)) => {
        self.abort.store(false, Ordering::Relaxed);

        let result = self
          .executor
          .evaluate_with_abort(&eval_allocator, expr, self.show_steps, self.abort);

        match result {
          None => println!("Interrupted"),
          Some(result) => println!("{result:#}"),
        }
      },

      Err(e) => println!("{e}"),
    }
  }
}

fn strip_prefix<'a>(input: &'a str, prefix: &str) -> &'a str {
  let s = input.trim();
  s.strip_prefix(prefix).unwrap_or(s).trim_start()
}
