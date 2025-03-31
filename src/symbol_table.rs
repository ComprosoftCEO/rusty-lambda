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
pub struct SymbolTable<'assign, 'eval>
where
  'assign: 'eval,
{
  assign_allocator: &'assign Allocator,
  eval_allocator: &'eval Allocator,

  globals: &'assign mut HashMap<&'assign str, ExprRef<'assign>>,
  assign_scopes: HashMap<&'assign str, u64>,
  eval_scopes: HashMap<&'eval str, u64>,

  messages: CompilerMessages,
}

impl<'assign, 'eval> SymbolTable<'assign, 'eval> {
  pub fn new(
    assign_allocator: &'assign Allocator,
    eval_allocator: &'eval Allocator,
    globals: &'assign mut HashMap<&'assign str, ExprRef<'assign>>,
  ) -> Self {
    Self {
      assign_allocator,
      eval_allocator,
      globals,
      assign_scopes: HashMap::new(),
      eval_scopes: HashMap::new(),
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
    if let Some(de_bruijn_index) = self.assign_scopes.get(name).cloned() {
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
    if self.assign_scopes.contains_key(name) {
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

    self.assign_scopes.entry(name).and_modify(|c| *c += 1).or_insert(1);
  }

  pub fn build_assign_lambda(&mut self, names: Vec<&'assign str>, body: ExprRef<'assign>) -> ExprRef<'assign> {
    names.into_iter().rev().fold(body, |body, name| {
      let new_count = self.assign_scopes.entry(name).and_modify(|c| *c -= 1).or_default();
      if *new_count == 0 {
        self.assign_scopes.remove(name);
      }

      self.assign_allocator.new_lambda(name, body)
    })
  }

  pub fn build_assign_eval(&mut self, left: ExprRef<'assign>, params: Vec<ExprRef<'assign>>) -> ExprRef<'assign> {
    params
      .into_iter()
      .fold(left, |left, right| self.assign_allocator.new_eval(left, right))
  }

  // ====================================
  //     Evaluations -- Shorter lifetime
  // ====================================

  pub fn build_eval_term(&mut self, name: &'assign str, offset: Offset) -> ExprRef<'eval> {
    if let Some(de_bruijn_index) = self.eval_scopes.get(name).cloned() {
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
    if self.eval_scopes.contains_key(name) {
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

    self.eval_scopes.entry(name).and_modify(|c| *c += 1).or_insert(1);
  }

  pub fn build_eval_lambda(&mut self, names: Vec<&'assign str>, body: ExprRef<'eval>) -> ExprRef<'eval> {
    names.into_iter().rev().fold(body, |body, name| {
      let new_count = self.eval_scopes.entry(name).and_modify(|c| *c -= 1).or_default();
      if *new_count == 0 {
        self.eval_scopes.remove(name);
      }

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
