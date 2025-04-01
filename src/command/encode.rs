use clap::{
  Args,
  builder::{ArgPredicate, NonEmptyStringValueParser},
};
use std::{fs, io::Write, num::NonZero, path::PathBuf};
use typed_arena::Arena;

use crate::expr::{Allocator, ExprRef, ExprVisitor};

use super::executor::Executor;

/// Encode an expression to Binary Lambda Calculus
#[derive(Args)]
pub struct EncodeArgs {
  /// Name of the term to encode
  #[clap(short, long)]
  term: String,

  /// List of files to load
  files: Vec<PathBuf>,

  /// Evaluate the term first before encoding it
  #[clap(short, long)]
  evaluate: bool,

  /// Output as raw bytes instead
  #[clap(short, long, group = "format")]
  binary: bool,

  /// Output as zero-width unicode characters
  #[clap(short, long, group = "format")]
  zero_width: bool,

  /// Character to output for a "0"
  #[clap(
    long,
    value_parser = NonEmptyStringValueParser::new(),
    conflicts_with = "binary",
    default_value = "0",
    default_value_if("zero_width", ArgPredicate::Equals("true".into()), Some("\u{ffa0}"))
  )]
  zero: String,

  /// Character output for a "1"
  #[clap(
    long,
    value_parser = NonEmptyStringValueParser::new(),
    conflicts_with = "binary",
    default_value = "1",
    default_value_if("zero_width", ArgPredicate::Equals("true".into()), Some("\u{3164}"))
  )]
  one: String,
}

impl EncodeArgs {
  pub fn execute(self) -> super::CommandResult {
    // Sanity check
    if self.zero == self.one {
      return Err("--zero and --one must be different values".into());
    }

    let text_data = Arena::new();
    let executor = Executor::new();

    // Load the prelude
    {
      let prelude = text_data.alloc(crate::PRELUDE.to_string());
      executor.load_code(prelude.as_str(), Some("prelude"))?;
    }

    // Load, but don't evaluate the code files
    for file in self.files.iter() {
      let file_data = text_data.alloc(fs::read_to_string(file)?);
      executor.load_code(file_data.as_str(), file.to_str())?;
    }

    // Look up the term by name
    let mut expr = match executor.get_global(&self.term) {
      None => return Err(format!("unknown term {}", self.term).into()),
      Some(expr) => expr,
    };

    // Possibly evaluate the expression
    let eval_allocator = Allocator::new();
    if self.evaluate {
      expr = executor.evaluate(&eval_allocator, expr);
    }

    if self.binary {
      // Binary encode the expression
      let mut visitor = ByteVisitor::new();
      expr.visit(&mut visitor);

      let bytes = visitor.into_bytes();
      std::io::stdout().write_all(&bytes)?;
    } else {
      // String encode the expression
      expr.visit(&mut PrintVisitor::new(&self.zero, &self.one));
      if !self.zero_width {
        println!();
      }
    }

    Ok(())
  }
}

/// Encode as a string
struct PrintVisitor<'zero, 'one> {
  zero: &'zero str,
  one: &'one str,
}

impl<'zero, 'one> PrintVisitor<'zero, 'one> {
  pub fn new(zero: &'zero str, one: &'one str) -> Self {
    Self { zero, one }
  }
}

impl<'eval> ExprVisitor<'eval> for PrintVisitor<'_, '_> {
  type Output = ();

  fn visit_term(&mut self, de_bruijn_index: NonZero<u64>) -> Self::Output {
    for _ in 0..de_bruijn_index.get() {
      print!("{}", self.one);
    }
    print!("{}", self.zero);
  }

  fn visit_lambda(&mut self, body: ExprRef<'eval>, _: &'eval str) -> Self::Output {
    print!("{}{}", self.zero, self.zero);
    body.visit(self);
  }

  fn visit_eval(&mut self, left: ExprRef<'eval>, right: ExprRef<'eval>) -> Self::Output {
    print!("{}{}", self.zero, self.one);
    left.visit(self);
    right.visit(self);
  }
}

/// Encode to a raw byte array
struct ByteVisitor {
  bits: Vec<u8>,
  bytes: Vec<u8>,
}

impl ByteVisitor {
  pub fn new() -> Self {
    Self {
      bits: Vec::new(),
      bytes: Vec::new(),
    }
  }

  pub fn into_bytes(mut self) -> Vec<u8> {
    // Pad the remaining space with 0's
    while !self.bits.is_empty() {
      self.push_bit(false);
    }

    self.bytes
  }

  fn push_bit(&mut self, bit: bool) {
    self.bits.push(if bit { 1 } else { 0 });

    if self.bits.len() == 8 {
      let byte = self.bits.drain(..).fold(0u8, |acc, bit| (acc << 1) | bit);
      self.bytes.push(byte);
    }
  }
}

impl<'eval> ExprVisitor<'eval> for ByteVisitor {
  type Output = ();

  fn visit_term(&mut self, de_bruijn_index: NonZero<u64>) -> Self::Output {
    for _ in 0..de_bruijn_index.get() {
      self.push_bit(true);
    }
    self.push_bit(false);
  }

  fn visit_lambda(&mut self, body: ExprRef<'eval>, _: &'eval str) -> Self::Output {
    self.push_bit(false);
    self.push_bit(false);
    body.visit(self);
  }

  fn visit_eval(&mut self, left: ExprRef<'eval>, right: ExprRef<'eval>) -> Self::Output {
    self.push_bit(false);
    self.push_bit(true);
    left.visit(self);
    right.visit(self);
  }
}
