use std::cell::{Ref, RefCell};
use std::collections::{self, BTreeMap};
use std::num::NonZero;
use std::sync::atomic::{AtomicBool, Ordering};
use std::{
  collections::{HashMap, btree_map},
  error::Error,
};

use crate::expr::{Allocator, ExprRef, ExprVisitor, UnpackedExpr};
use crate::lambda::{EvalExpressionParser as ExpressionParser, ProgramParser, StatementParser};
use crate::symbol_table::SymbolTable;

pub struct Executor<'s> {
  assign_allocator: Allocator,
  globals: RefCell<BTreeMap<&'s str, ExprRef<'s>>>,
  numbers: RefCell<Vec<ExprRef<'s>>>,
  program_parser: ProgramParser,
  statement_parser: StatementParser,
  expression_parser: ExpressionParser,
}

impl<'s> Executor<'s> {
  pub fn new() -> Self {
    Self {
      assign_allocator: Allocator::new(),
      globals: RefCell::new(BTreeMap::new()),
      numbers: RefCell::new(Vec::new()),
      program_parser: ProgramParser::new(),
      statement_parser: StatementParser::new(),
      expression_parser: ExpressionParser::new(),
    }
  }

  #[inline]
  #[allow(unused)]
  pub fn get_global(&self, name: &str) -> Option<ExprRef<'s>> {
    self.globals.borrow().get(name).cloned()
  }

  #[inline]
  pub fn all_globals(&self) -> &RefCell<BTreeMap<&'s str, ExprRef<'s>>> {
    &self.globals
  }

  /// Load a code file and return any statements that might need to be evaluated.
  /// Name is just a helpful string for error handling.
  pub fn load_code(&'s self, code: &'s str, name: Option<&str>) -> Result<Vec<ExprRef<'s>>, Box<dyn Error>> {
    let name_str = name.map(|n| format!("{n}: ")).unwrap_or_default();

    let mut globals = self.globals.borrow_mut();
    let mut numbers = self.numbers.borrow_mut();

    let mut symbol_table = SymbolTable::new(
      &self.assign_allocator,
      &self.assign_allocator,
      &mut globals,
      &mut numbers,
    );
    symbol_table.set_line_numbers(code);

    let results = self
      .program_parser
      .parse(&mut symbol_table, code)
      .map_err(|e| format!("{name_str}parsing error: {e}"))?;

    symbol_table.print_messages();
    if symbol_table.has_errors() {
      return Err(format!("{name_str}failed to load code").into());
    }

    Ok(results)
  }

  /// Load a single statement. Returns None if an assignment was loaded instead.
  pub fn load_statement<'eval>(
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

    Ok(result)
  }

  /// Load a single expression.
  pub fn load_expression<'eval>(
    &'s self,
    eval_allocator: &'eval Allocator,
    code: &'s str,
  ) -> Result<ExprRef<'eval>, Box<dyn Error>>
  where
    's: 'eval,
  {
    let mut globals = self.globals.borrow_mut();
    let mut numbers = self.numbers.borrow_mut();

    let mut symbol_table = SymbolTable::new(&self.assign_allocator, eval_allocator, &mut globals, &mut numbers);
    let result = self
      .expression_parser
      .parse(&mut symbol_table, code)
      .map_err(|e| format!("parsing error: {e}"))?;

    symbol_table.print_messages();
    if symbol_table.has_errors() {
      return Err("failed to evaluate expression".into());
    }

    Ok(result)
  }

  /// Evaluate an expression and return the result.
  pub fn evaluate<'eval>(
    &self,
    eval_allocator: &'eval Allocator,
    expr: ExprRef<'eval>,
    show_steps: bool,
  ) -> ExprRef<'eval>
  where
    's: 'eval,
  {
    Evaluator::new(eval_allocator, show_steps).evaluate(expr)
  }

  /// Returns `None` if aborted with Ctrl+C
  pub fn evaluate_with_abort<'eval>(
    &self,
    eval_allocator: &'eval Allocator,
    expr: ExprRef<'eval>,
    show_steps: bool,
    abort: &AtomicBool,
  ) -> Option<ExprRef<'eval>> {
    Evaluator::new(eval_allocator, show_steps).evaluate_with_abort(expr, abort)
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

  fn visit_term(&mut self, expr: ExprRef<'eval>, de_bruijn_index: NonZero<u64>) -> Self::Output {
    if de_bruijn_index.get() < self.cutoff {
      expr // Optimization: avoid an extra allocation
    } else {
      let new_de_bruijn_index = NonZero::new((de_bruijn_index.get() as i64 + self.offset) as u64);
      self.eval_allocator.new_term(new_de_bruijn_index.expect("index is 0"))
    }
  }

  fn visit_lambda(&mut self, expr: ExprRef<'eval>, body: ExprRef<'eval>, parameter_name: &'eval str) -> Self::Output {
    self.cutoff += 1;
    let new_body = body.visit(self);
    self.cutoff -= 1;

    if new_body == body {
      expr // Optimization: avoid an extra allocation
    } else {
      self.eval_allocator.new_lambda(parameter_name, new_body)
    }
  }

  fn visit_eval(&mut self, expr: ExprRef<'eval>, left: ExprRef<'eval>, right: ExprRef<'eval>) -> Self::Output {
    let new_left = left.visit(self);
    let new_right = right.visit(self);

    if new_left == left && new_right == right {
      expr // Optimization: avoid an extra allocation
    } else {
      self.eval_allocator.new_eval(new_left, new_right)
    }
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

  fn visit_term(&mut self, expr: ExprRef<'eval>, de_bruijn_index: NonZero<u64>) -> Self::Output {
    if de_bruijn_index.get() == self.target {
      self.get_offset_expr(self.target)
    } else {
      expr // Optimization: avoid an extra allocation
    }
  }

  fn visit_lambda(&mut self, expr: ExprRef<'eval>, body: ExprRef<'eval>, parameter_name: &'eval str) -> Self::Output {
    self.target += 1;
    let new_body = body.visit(self);
    self.target -= 1;

    if new_body == body {
      expr // Optimization: avoid an extra allocation
    } else {
      self.eval_allocator.new_lambda(parameter_name, new_body)
    }
  }

  fn visit_eval(&mut self, expr: ExprRef<'eval>, left: ExprRef<'eval>, right: ExprRef<'eval>) -> Self::Output {
    let new_left = left.visit(self);
    let new_right = right.visit(self);

    if new_left == left && new_right == right {
      expr // Optimization: avoid an extra allocation
    } else {
      self.eval_allocator.new_eval(new_left, new_right)
    }
  }
}

struct Evaluator<'eval> {
  eval_allocator: &'eval Allocator,
  show_steps: bool,
  something_changed: bool,
}

impl<'eval> Evaluator<'eval> {
  pub fn new(eval_allocator: &'eval Allocator, show_steps: bool) -> Self {
    Self {
      eval_allocator,
      show_steps,
      something_changed: false,
    }
  }

  /// Recursively evaluate the lambda expression
  pub fn evaluate(&mut self, mut expr: ExprRef<'eval>) -> ExprRef<'eval> {
    for step in 0u64.. {
      if self.show_steps {
        eprintln!("{step}: {expr:#}");
      }

      self.something_changed = false;
      expr = self.evaluate_strong(expr);

      if !self.something_changed {
        break;
      }
    }

    expr
  }

  /// Same as evaluate(), but has an atomic boolean that can be used to abort early by setting to `true`
  pub fn evaluate_with_abort(&mut self, mut expr: ExprRef<'eval>, abort: &AtomicBool) -> Option<ExprRef<'eval>> {
    for step in 0u64.. {
      if self.show_steps {
        eprintln!("{step}: {expr:#}");
      }

      if abort.load(Ordering::Relaxed) {
        return None;
      }

      self.something_changed = false;
      expr = self.evaluate_strong(expr);

      if !self.something_changed {
        break;
      }
    }

    Some(expr)
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
        if new_left != left {
          return self.eval_allocator.new_eval(new_left, right);
        }

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
        if new_left != left {
          return self.eval_allocator.new_eval(new_left, right);
        }

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
