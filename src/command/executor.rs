use std::cell::RefCell;
use std::num::NonZero;
use std::{collections::HashMap, error::Error};

use crate::expr::{Allocator, ExprRef, ExprVisitor, UnpackedExpr};
use crate::lambda::{ProgramParser, StatementParser};
use crate::symbol_table::SymbolTable;

pub struct Executor<'s> {
  assign_allocator: Allocator,
  globals: RefCell<HashMap<&'s str, ExprRef<'s>>>,
  numbers: RefCell<Vec<ExprRef<'s>>>,
  program_parser: ProgramParser,
  statement_parser: StatementParser,
}

impl<'s> Executor<'s> {
  pub fn new() -> Self {
    Self {
      assign_allocator: Allocator::new(),
      globals: RefCell::new(HashMap::new()),
      numbers: RefCell::new(Vec::new()),
      program_parser: ProgramParser::new(),
      statement_parser: StatementParser::new(),
    }
  }

  #[inline]
  pub fn get_global(&self, name: &str) -> Option<ExprRef<'s>> {
    self.globals.borrow().get(name).cloned()
  }

  /// Load a code file, but don't evaluate anything.
  /// Name is just a helpful string for error handling.
  pub fn load_code(&'s self, code: &'s str, name: Option<&str>) -> Result<(), Box<dyn Error>> {
    let name_str = name.map(|n| format!("{n}: ")).unwrap_or_default();

    let eval_allocator = Allocator::new();
    let mut globals = self.globals.borrow_mut();
    let mut numbers = self.numbers.borrow_mut();

    let mut symbol_table = SymbolTable::new(&self.assign_allocator, &eval_allocator, &mut globals, &mut numbers);
    symbol_table.set_line_numbers(code);

    self
      .program_parser
      .parse(&mut symbol_table, code)
      .map_err(|e| format!("{name_str}parsing error: {e}"))?;

    symbol_table.print_messages();
    if symbol_table.has_errors() {
      return Err(format!("{name_str}failed to load code").into());
    }

    Ok(())
  }

  /// Load a code file and evaluate the statement
  pub fn load_and_eval_code<'eval>(
    &'s self,
    eval_allocator: &'eval Allocator,
    code: &'s str,
    name: Option<&str>,
  ) -> Result<impl Iterator<Item = ExprRef<'eval>>, Box<dyn Error>>
  where
    's: 'eval,
  {
    let name_str = name.map(|n| format!("{n}: ")).unwrap_or_default();

    let mut globals = self.globals.borrow_mut();
    let mut numbers = self.numbers.borrow_mut();

    let mut symbol_table = SymbolTable::new(&self.assign_allocator, eval_allocator, &mut globals, &mut numbers);
    let results = self
      .program_parser
      .parse(&mut symbol_table, code)
      .map_err(|e| format!("{name_str}parsing error: {e}"))?;

    symbol_table.print_messages();
    if symbol_table.has_errors() {
      return Err(format!("{name_str}failed to load code").into());
    }

    Ok(
      results
        .into_iter()
        .map(|result| Evaluator::new(eval_allocator).evaluate(result)),
    )
  }

  /// Load and evaluate a single statement
  pub fn load_and_eval_statement<'eval>(
    &'s self,
    eval_allocator: &'eval Allocator,
    code: &'s str,
  ) -> Result<Option<ExprRef<'eval>>, Box<dyn Error>>
  where
    's: 'eval,
  {
    let mut globals = self.globals.borrow_mut();
    let mut numbers = self.numbers.borrow_mut();

    let mut symbol_table = SymbolTable::new(&self.assign_allocator, eval_allocator, &mut globals, &mut numbers);
    let result = self
      .statement_parser
      .parse(&mut symbol_table, code)
      .map_err(|e| format!("parsing error: {e}"))?;

    symbol_table.print_messages();
    if symbol_table.has_errors() {
      return Err("failed to evaluate statement".into());
    }

    Ok(result.map(|result| Evaluator::new(eval_allocator).evaluate(result)))
  }

  pub fn evaluate<'eval>(&self, eval_allocator: &'eval Allocator, expr: ExprRef<'eval>) -> ExprRef<'eval>
  where
    's: 'eval,
  {
    Evaluator::new(eval_allocator).evaluate(expr)
  }
}

struct Shift<'eval> {
  eval_allocator: &'eval Allocator,
  cutoff: u64,
  offset: i64,
}

impl<'eval> Shift<'eval> {
  pub fn new(eval_allocator: &'eval Allocator, cutoff: u64, offset: i64) -> Self {
    Self {
      eval_allocator,
      cutoff,
      offset,
    }
  }
}

impl<'eval> ExprVisitor<'eval> for Shift<'eval> {
  type Output = ExprRef<'eval>;

  fn visit_term(&mut self, de_bruijn_index: NonZero<u64>) -> Self::Output {
    if de_bruijn_index.get() < self.cutoff {
      self.eval_allocator.new_term(de_bruijn_index)
    } else {
      let new_de_bruijn_index = NonZero::new((de_bruijn_index.get() as i64 + self.offset) as u64);
      self.eval_allocator.new_term(new_de_bruijn_index.expect("index is 0"))
    }
  }

  fn visit_lambda(&mut self, body: ExprRef<'eval>, parameter_name: &'eval str) -> Self::Output {
    self.cutoff += 1;
    let new_body = body.visit(self);
    self.cutoff -= 1;

    self.eval_allocator.new_lambda(parameter_name, new_body)
  }

  fn visit_eval(&mut self, left: ExprRef<'eval>, right: ExprRef<'eval>) -> Self::Output {
    let new_left = left.visit(self);
    let new_right = right.visit(self);
    self.eval_allocator.new_eval(new_left, new_right)
  }
}

struct Replace<'eval> {
  eval_allocator: &'eval Allocator,
  target: u64,
  default_expr: ExprRef<'eval>,
  offsets: HashMap<u64, ExprRef<'eval>>,
}

impl<'eval> Replace<'eval> {
  pub fn new(eval_allocator: &'eval Allocator, new_value: ExprRef<'eval>) -> Self {
    Self {
      eval_allocator,
      target: 1,
      default_expr: new_value,
      offsets: HashMap::from([(1, new_value)]),
    }
  }

  fn get_offset_expr(&mut self, offset: u64) -> ExprRef<'eval> {
    *self.offsets.entry(offset).or_insert_with(|| {
      self
        .default_expr
        .visit(&mut Shift::new(self.eval_allocator, 1, (offset as i64) - 1))
    })
  }
}

impl<'eval> ExprVisitor<'eval> for Replace<'eval> {
  type Output = ExprRef<'eval>;

  fn visit_term(&mut self, de_bruijn_index: NonZero<u64>) -> Self::Output {
    if de_bruijn_index.get() == self.target {
      self.get_offset_expr(self.target)
    } else {
      self.eval_allocator.new_term(de_bruijn_index)
    }
  }

  fn visit_lambda(&mut self, body: ExprRef<'eval>, parameter_name: &'eval str) -> Self::Output {
    self.target += 1;
    let new_body = body.visit(self);
    self.target -= 1;

    self.eval_allocator.new_lambda(parameter_name, new_body)
  }

  fn visit_eval(&mut self, left: ExprRef<'eval>, right: ExprRef<'eval>) -> Self::Output {
    let new_left = left.visit(self);
    let new_right = right.visit(self);
    self.eval_allocator.new_eval(new_left, new_right)
  }
}

struct Evaluator<'eval> {
  eval_allocator: &'eval Allocator,
  something_changed: bool,
}

impl<'eval> Evaluator<'eval> {
  pub fn new(eval_allocator: &'eval Allocator) -> Self {
    Self {
      eval_allocator,
      something_changed: false,
    }
  }

  /// Recursively evaluate the lambda expression
  pub fn evaluate(&mut self, mut expr: ExprRef<'eval>) -> ExprRef<'eval> {
    loop {
      self.something_changed = false;
      expr = self.evaluate_strong(expr);

      if !self.something_changed {
        return expr;
      }
    }
  }

  /// Attempts to evaluate the body of a lambda expression
  fn evaluate_strong(&mut self, expr: ExprRef<'eval>) -> ExprRef<'eval> {
    use UnpackedExpr::*;

    match expr.unpack() {
      Term { .. } => expr,

      Lambda { body, parameter_name } => {
        let new_body = self.evaluate_strong(body);
        if new_body == body {
          expr // Optimization: avoid an extra allocation
        } else {
          self.eval_allocator.new_lambda(parameter_name, new_body)
        }
      },

      Eval { left, right } => {
        let new_left = self.evaluate_weak(left);
        match new_left.unpack() {
          Term { .. } | Eval { .. } => {
            let new_right = self.evaluate_strong(right);
            if new_left == left && new_right == right {
              expr // Optimization: avoid an extra allocation
            } else {
              self.eval_allocator.new_eval(new_left, new_right)
            }
          },

          Lambda { body, .. } => {
            self.something_changed = true;

            let shifted_right = right.visit(&mut Shift::new(self.eval_allocator, 1, 1));
            body
              .visit(&mut Replace::new(self.eval_allocator, shifted_right))
              .visit(&mut Shift::new(self.eval_allocator, 1, -1))
            // No need to recurse ... next loop iteration will attempt the substitution
          },
        }
      },
    }
  }

  /// Lambda expression is left as lazily evaluated
  fn evaluate_weak(&mut self, expr: ExprRef<'eval>) -> ExprRef<'eval> {
    use UnpackedExpr::*;

    match expr.unpack() {
      Term { .. } => expr,

      Lambda { .. } => expr, // Lazily evaluated

      Eval { left, right } => {
        let new_left = self.evaluate_weak(left);
        match new_left.unpack() {
          Term { .. } | Eval { .. } => {
            let new_right = self.evaluate_strong(right);
            if new_left == left && new_right == right {
              expr // Optimization: avoid an extra allocation
            } else {
              self.eval_allocator.new_eval(new_left, new_right)
            }
          },

          Lambda { body, .. } => {
            self.something_changed = true;

            let shifted_right = right.visit(&mut Shift::new(self.eval_allocator, 1, 1));
            body
              .visit(&mut Replace::new(self.eval_allocator, shifted_right))
              .visit(&mut Shift::new(self.eval_allocator, 1, -1))
            // No need to recurse ... next loop iteration will attempt the substitution
          },
        }
      },
    }
  }
}
