use crate::expr::{Allocator, ExprRef};
use crossterm::style::Stylize;
use itertools::Itertools;
use lalrpop_util::{ErrorRecovery, lexer::Token};
use num_traits::Num;
use std::{
  borrow::Cow,
  collections::{BTreeMap, HashMap},
  fmt,
  num::NonZero,
};

/// - Assigning an expression keeps results allocated permanently.
/// - Evaluating an expression only computes results then clears allocations.
pub struct SymbolTable<'assign, 'eval, 'globals, 'numbers>
where
  'assign: 'eval,
{
  assign_allocator: &'assign Allocator,
  eval_allocator: &'eval Allocator,

  globals: &'globals mut HashMap<&'assign str, ExprRef<'assign>>,
  numbers: &'numbers mut Vec<ExprRef<'assign>>,
  assign_scopes: Vec<&'assign str>,
  eval_scopes: Vec<&'eval str>,

  messages: CompilerMessages,
}

impl<'assign, 'eval, 'globals, 'numbers> SymbolTable<'assign, 'eval, 'globals, 'numbers> {
  pub fn new(
    assign_allocator: &'assign Allocator,
    eval_allocator: &'eval Allocator,
    globals: &'globals mut HashMap<&'assign str, ExprRef<'assign>>,
    numbers: &'numbers mut Vec<ExprRef<'assign>>,
  ) -> Self {
    Self {
      assign_allocator,
      eval_allocator,
      globals,
      numbers,
      assign_scopes: Vec::new(),
      eval_scopes: Vec::new(),
      messages: CompilerMessages::new(),
    }
  }

  pub fn get_compiler_messages(&self) -> &Vec<CompilerMessage> {
    &self.messages.messages
  }

  pub fn has_errors(&self) -> bool {
    self.messages.has_errors()
  }

  pub fn print_messages(&self) {
    self.messages.messages.iter().for_each(CompilerMessage::print);
  }

  pub fn parse_error(&mut self, parse_error: ErrorRecovery<usize, Token<'assign>, &'static str>) {
    self.messages.parse_error(parse_error);
  }

  /// Figure out the line numbers before attempting to compile a program
  pub fn set_line_numbers(&mut self, full_program: &str) {
    self.messages.set_offset_map(
      full_program
        .lines()
        .zip(1usize..)
        .map(|(line_str, line_number)| (line_str.as_ptr() as usize - full_program.as_ptr() as usize, line_number))
        .collect(),
    )
  }

  // ====================================
  //     Assignments -- Long lifetime
  // ====================================

  pub fn declare_global(&mut self, name: &'assign str, expr: ExprRef<'assign>, offset: Offset) {
    if self.globals.contains_key(name) {
      return self.messages.error(format!("duplicate variable {name}"), Some(offset));
    }

    self.globals.insert(name, expr);
  }

  pub fn build_assign_term(&mut self, name: &'assign str, offset: Offset) -> ExprRef<'assign> {
    // O(n) search for the last time a term was used
    // (We can probably find a more efficient way to do this...)
    let found_index = self
      .assign_scopes
      .iter()
      .rev()
      .zip(1u64..)
      .filter_map(|(n, index)| (*n == name).then_some(index))
      .next();

    if let Some(de_bruijn_index) = found_index {
      // Parent scopes have the highest priority
      self
        .assign_allocator
        .new_term(NonZero::new(de_bruijn_index).expect("invalid index"))
    } else if let Some(global_expr) = self.globals.get(name) {
      // Global expressions are substituted verbatim
      *global_expr
    } else {
      self.messages.error(format!("unknown term: {name}"), Some(offset));

      // Term 1 is always valid, return it so we can continue parsing
      self.assign_allocator.new_term(unsafe { NonZero::new_unchecked(1) })
    }
  }

  pub fn start_assign_lambda(&mut self, name: &'assign str, offset: Offset) {
    // Show warnings (but not errors) about shadowed variables
    if self.assign_scopes.contains(&name) {
      self.messages.warning(
        format!("parameter {name} shadows outer parameter of the same name"),
        Some(offset),
      );
    } else if self.globals.contains_key(name) {
      self.messages.warning(
        format!("parameter {name} shadows variable of the same name"),
        Some(offset),
      );
    }

    self.assign_scopes.push(name);
  }

  pub fn build_assign_lambda(&mut self, names: Vec<&'assign str>, body: ExprRef<'assign>) -> ExprRef<'assign> {
    names.into_iter().rev().fold(body, |body, name| {
      self.assign_scopes.pop();
      self.assign_allocator.new_lambda(name, body)
    })
  }

  pub fn build_assign_eval(&mut self, left: ExprRef<'assign>, params: Vec<ExprRef<'assign>>) -> ExprRef<'assign> {
    params
      .into_iter()
      .fold(left, |left, right| self.assign_allocator.new_eval(left, right))
  }

  pub fn build_number(&mut self, number: u64) -> ExprRef<'assign> {
    // 0 should always exist in the list
    if self.numbers.is_empty() {
      self
        .numbers
        .push(self.assign_allocator.new_term(unsafe { NonZero::new_unchecked(1) }));
    }

    let mut lambda_number = self
      .numbers
      .get(number as usize)
      .or_else(|| self.numbers.last())
      .cloned()
      .unwrap();

    // Recursively build out the number (f (f (f x)))
    while (self.numbers.len() as u64) <= number {
      lambda_number = self.assign_allocator.new_eval(
        self.assign_allocator.new_term(unsafe { NonZero::new_unchecked(2) }),
        lambda_number,
      );
      self.numbers.push(lambda_number);
    }

    // Then wrap it in a lambda expression (\f.\x.(f (f (f x))))
    self
      .assign_allocator
      .new_lambda("f", self.assign_allocator.new_lambda("x", lambda_number))
  }

  // ====================================
  //     Evaluations -- Shorter lifetime
  // ====================================

  pub fn build_eval_term(&mut self, name: &'assign str, offset: Offset) -> ExprRef<'eval> {
    // O(n) search for the last time a term was used
    // (We can probably find a more efficient way to do this...)
    let found_index = self
      .eval_scopes
      .iter()
      .rev()
      .zip(1u64..)
      .filter_map(|(n, index)| (*n == name).then_some(index))
      .next();

    if let Some(de_bruijn_index) = found_index {
      // Parent scopes have the highest priority
      self
        .eval_allocator
        .new_term(NonZero::new(de_bruijn_index).expect("invalid index"))
    } else if let Some(global_expr) = self.globals.get(name) {
      // Global expressions are substituted verbatim
      *global_expr
    } else {
      self.messages.error(format!("unknown term: {name}"), Some(offset));

      // Term 1 is always valid, return it so we can continue parsing
      self.eval_allocator.new_term(unsafe { NonZero::new_unchecked(1) })
    }
  }

  pub fn start_eval_lambda(&mut self, name: &'assign str, offset: Offset) {
    // Show warnings (but not errors) about shadowed variables
    if self.eval_scopes.contains(&name) {
      self.messages.warning(
        format!("parameter {name} shadows outer parameter of the same name"),
        Some(offset),
      );
    } else if self.globals.contains_key(name) {
      self.messages.warning(
        format!("parameter {name} shadows variable of the same name"),
        Some(offset),
      );
    }

    self.eval_scopes.push(name);
  }

  pub fn build_eval_lambda(&mut self, names: Vec<&'assign str>, body: ExprRef<'eval>) -> ExprRef<'eval> {
    names.into_iter().rev().fold(body, |body, name| {
      self.eval_scopes.pop();
      self.eval_allocator.new_lambda(name, body)
    })
  }

  pub fn build_eval_eval(&mut self, left: ExprRef<'eval>, params: Vec<ExprRef<'eval>>) -> ExprRef<'eval> {
    params
      .into_iter()
      .fold(left, |left, right| self.eval_allocator.new_eval(left, right))
  }
}

#[derive(Debug, Clone, Default)]
struct CompilerMessages {
  messages: Vec<CompilerMessage>,
  offset_map: BTreeMap<usize, usize>, // Maps byte offset to line number
}

#[allow(unused)]
impl CompilerMessages {
  pub fn new() -> Self {
    Self {
      messages: vec![],
      offset_map: BTreeMap::new(),
    }
  }

  pub fn set_offset_map(&mut self, map: BTreeMap<usize, usize>) {
    self.offset_map = map;
  }

  pub fn has_errors(&self) -> bool {
    self.messages.iter().any(CompilerMessage::is_error)
  }

  pub fn warning<T: Into<Cow<'static, str>>>(&mut self, msg: T, offset: Option<Offset>) {
    self.messages.push(CompilerMessage::Warning {
      message: msg.into(),
      line_number: offset.and_then(|o| self.lookup_line_number(o.0)),
    });
  }

  pub fn error<T: Into<Cow<'static, str>>>(&mut self, msg: T, offset: Option<Offset>) {
    self.messages.push(CompilerMessage::Error {
      message: msg.into(),
      line_number: offset.and_then(|o| self.lookup_line_number(o.0)),
    });
  }

  pub fn parse_error(&mut self, parse_error: ErrorRecovery<usize, Token<'_>, &'static str>) {
    let error = parse_error
      .error
      .map_location(|l| self.lookup_line_number(l).unwrap_or(LineNumber::new(l)));

    self.error(format!("{}", error), None);
  }

  pub fn print_messages(&self) {
    self.messages.iter().for_each(CompilerMessage::print);
  }

  fn lookup_line_number(&self, offset: usize) -> Option<LineNumber> {
    self
      .offset_map
      .range(..=offset)
      .last()
      .map(|(key, value)| LineNumber::new_with_offset(*value, offset - *key))
  }
}

#[derive(Debug, Clone)]
pub enum CompilerMessage {
  Warning {
    message: Cow<'static, str>,
    line_number: Option<LineNumber>,
  },

  Error {
    message: Cow<'static, str>,
    line_number: Option<LineNumber>,
  },
}

impl CompilerMessage {
  pub fn is_warning(&self) -> bool {
    matches!(self, Self::Warning { .. })
  }

  pub fn is_error(&self) -> bool {
    matches!(self, Self::Error { .. })
  }

  pub fn message(&self) -> &Cow<'static, str> {
    match self {
      Self::Warning { message, .. } => message,
      Self::Error { message, .. } => message,
    }
  }

  pub fn line_number(&self) -> Option<LineNumber> {
    match self {
      Self::Warning { line_number, .. } => *line_number,
      Self::Error { line_number, .. } => *line_number,
    }
  }

  pub fn print(&self) {
    let (prefix, message, line_number) = match self {
      Self::Warning { message, line_number } => ("Warning".yellow(), message, line_number),
      Self::Error { message, line_number } => ("Error".red(), message, line_number),
    };

    let message = if let Some(line_number) = line_number {
      match message.lines().collect_vec()[..] {
        [] => format!("(on line {line_number})"),
        [message] => format!("{message} (on line {line_number})"),
        [message, ref rest @ ..] => format!("{message} (on line {line_number})\n{}", rest.join("\n")),
      }
    } else {
      message.to_string()
    };

    println!("{prefix}: {message}");
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LineNumber {
  pub line: usize,
  pub offset: Option<usize>,
}

impl LineNumber {
  pub fn new(line: usize) -> Self {
    Self { line, offset: None }
  }

  pub fn new_with_offset(line: usize, offset: usize) -> Self {
    Self {
      line,
      offset: Some(offset),
    }
  }
}

impl fmt::Display for LineNumber {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    if let Some(offset) = self.offset {
      format!("{}:{}", self.line, offset)
    } else {
      format!("{}", self.line)
    }
    .fmt(f)
  }
}

/// Store a byte index inside the source code for better error handling.
/// Define it as a new type variable so the compiler enforces type safety.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Offset(pub usize);

impl From<usize> for Offset {
  fn from(other: usize) -> Self {
    Self(other)
  }
}

impl From<Offset> for usize {
  fn from(other: Offset) -> Self {
    other.0
  }
}

/// Convert an integer literal string into an integer
pub fn parse_integer_literal<T: Num>(input: &str) -> Result<T, T::FromStrRadixErr> {
  // Filter any underscore characters
  let input: String = input.chars().filter(|c| *c != '_').collect();

  T::from_str_radix(&input, 10)
}
