use clap::{
  Args,
  builder::{ArgPredicate, NonEmptyStringValueParser},
};
use std::{fs, io::Read, num::NonZero, path::PathBuf};
use typed_arena::Arena;

use crate::{
  command::executor::Executor,
  expr::{Allocator, ExprRef},
};

/// Decode a Binary Lambda Calculus expression
#[derive(Args)]
pub struct DecodeArgs {
  /// File to decode. Reads from stdin if omitted.
  file: Option<PathBuf>,

  /// Treat as a binary file instead of text
  #[clap(short, long, group = "format")]
  binary: bool,

  /// Parse input as zero-width unicode characters
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

  /// Evaluate the term after decoding it
  #[clap(short, long)]
  evaluate: bool,
}

impl DecodeArgs {
  pub fn execute(self) -> super::CommandResult {
    // Sanity check
    if self.zero == self.one {
      return Err("--zero and --one must be different values".into());
    }

    // Read from either a file or stdin
    let mut reader: Box<dyn Read> = match self.file {
      None => Box::new(std::io::stdin()),
      Some(f) => Box::new(fs::File::open(f)?),
    };

    let mut s = String::new();
    let mut bit_iter: Box<dyn Iterator<Item = bool>> = if self.binary {
      // Parse as binary
      let mut bytes = Vec::new();
      reader.read_to_end(&mut bytes)?;
      Box::new(get_byte_iter(bytes))
    } else {
      // Parse as text
      reader.read_to_string(&mut s)?;
      Box::new(Extractor::new(&self.zero, &self.one, &s))
    };

    let text_data = Arena::new();
    let allocator = Allocator::new();

    let mut decoder = Decoder::new(&text_data, &allocator);
    let mut expr = match decoder.decode_expr(&mut bit_iter) {
      None => return Err("failed to decode lambda expression".into()),
      Some(expr) => expr,
    };

    // Possibly evaluate the expression
    if self.evaluate {
      let executor = Executor::new();
      expr = executor.evaluate(&allocator, expr);
    }

    // Print decoded expression
    println!("{expr}");

    Ok(())
  }
}

#[inline]
fn get_byte_iter(bytes: Vec<u8>) -> impl Iterator<Item = bool> {
  bytes.into_iter().flat_map(to_bits_iter)
}

#[inline]
fn to_bits_iter(byte: u8) -> impl Iterator<Item = bool> {
  (0..=7).rev().map(move |s| (byte >> s) & 1 == 1)
}

struct Extractor<'zero, 'one, 's> {
  zero: &'zero str,
  one: &'one str,
  s: &'s str,
}

impl<'zero, 'one, 's> Extractor<'zero, 'one, 's> {
  pub fn new(zero: &'zero str, one: &'one str, s: &'s str) -> Self {
    Self { zero, one, s }
  }
}

impl Iterator for Extractor<'_, '_, '_> {
  type Item = bool;

  fn next(&mut self) -> Option<Self::Item> {
    // O(2*n) inefficient but I don't care algorithm is simple
    let next_zero = self.s.find(self.zero);
    let next_one = self.s.find(self.one);

    match (next_zero, next_one) {
      (None, None) => None,
      (Some(zero), None) => {
        self.s = &self.s[(zero + self.zero.len())..];
        Some(false)
      },
      (None, Some(one)) => {
        self.s = &self.s[(one + self.one.len())..];
        Some(true)
      },
      (Some(zero), Some(one)) if zero < one => {
        self.s = &self.s[(zero + self.zero.len())..];
        Some(false)
      },
      (Some(_), Some(one)) => {
        self.s = &self.s[(one + self.one.len())..];
        Some(true)
      },
    }
  }
}

struct Decoder<'alloc> {
  text_data: &'alloc Arena<String>,
  allocator: &'alloc Allocator,
  variable_names: Vec<&'alloc str>,
  current_scope: u64,
}

impl<'alloc> Decoder<'alloc> {
  pub fn new(text_data: &'alloc Arena<String>, allocator: &'alloc Allocator) -> Self {
    Self {
      text_data,
      allocator,
      variable_names: Vec::new(),
      current_scope: 0,
    }
  }

  fn get_parameter_name(&mut self) -> &'alloc str {
    for i in self.variable_names.len()..=(self.current_scope as usize) {
      let data = self.text_data.alloc(format!("x{}", i + 1));
      self.variable_names.push(data.as_str());
    }

    self.variable_names[(self.current_scope - 1) as usize]
  }

  pub fn decode_expr(&mut self, iter: &mut dyn Iterator<Item = bool>) -> Option<ExprRef<'alloc>> {
    match iter.next() {
      None => {
        println!("failed to decode expression: unexpected end of input");
        None
      },
      Some(false) => match iter.next() {
        None => {
          println!("failed to decode expression: unexpected end of input");
          None
        },
        Some(false) => self.decode_lambda(iter),
        Some(true) => self.decode_eval(iter),
      },
      Some(true) => self.decode_term(iter),
    }
  }

  fn decode_term(&mut self, iter: &mut dyn Iterator<Item = bool>) -> Option<ExprRef<'alloc>> {
    let mut term_index = 1;
    loop {
      match iter.next() {
        None => {
          println!("failed to decode term: unexpected end of input");
          return None;
        },
        Some(true) => {
          term_index += 1;
        },
        Some(false) => {
          break;
        },
      }
    }

    if term_index > self.current_scope {
      println!(
        "invalid term: index {term_index} > current lambda index {}",
        self.current_scope
      );
      None
    } else {
      let term = self
        .allocator
        .new_term(NonZero::new(term_index).expect("index is zero"));
      Some(term)
    }
  }

  fn decode_lambda(&mut self, iter: &mut dyn Iterator<Item = bool>) -> Option<ExprRef<'alloc>> {
    self.current_scope += 1;
    let body = self.decode_expr(iter)?;
    let param_name = self.get_parameter_name();
    self.current_scope -= 1;

    let lambda = self.allocator.new_lambda(param_name, body);
    Some(lambda)
  }

  fn decode_eval(&mut self, iter: &mut dyn Iterator<Item = bool>) -> Option<ExprRef<'alloc>> {
    let left = self.decode_expr(iter)?;
    let right = self.decode_expr(iter)?;
    let eval = self.allocator.new_eval(left, right);
    Some(eval)
  }
}
