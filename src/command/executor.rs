use std::cell::RefCell;
use std::{collections::HashMap, error::Error};

use crate::expr::{Allocator, ExprRef};
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

  /// Load a code file, but don't evaluate anything.
  /// Name is just a helpful string for error handling.
  pub fn load_code(&'s self, code: &'s str, name: Option<&str>) -> Result<(), Box<dyn Error>> {
    let name_str = name.map(|n| format!("{n}: ")).unwrap_or_default();

    let eval_allocator = Allocator::new();

    // Safety: the compiler isn't smart enough to know that this borrow doesn't need to outlive the function
    let mut globals = self.globals.borrow_mut();
    let globals_ptr: &'s mut HashMap<&'s str, ExprRef<'s>> = unsafe { &mut *&raw mut globals };

    let mut numbers = self.numbers.borrow_mut();
    let numbers_ptr: &'s mut Vec<ExprRef<'s>> = unsafe { &mut *&raw mut numbers };

    let mut symbol_table = SymbolTable::new(&self.assign_allocator, &eval_allocator, globals_ptr, numbers_ptr);
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
  ) -> Result<Vec<ExprRef<'eval>>, Box<dyn Error>>
  where
    's: 'eval,
  {
    let name_str = name.map(|n| format!("{n}: ")).unwrap_or_default();

    // Safety: the compiler isn't smart enough to know that this borrow doesn't need to outlive the function
    let mut globals = self.globals.borrow_mut();
    let globals_ptr: &'s mut HashMap<&'s str, ExprRef<'s>> = unsafe { &mut *&raw mut globals };

    let mut numbers = self.numbers.borrow_mut();
    let numbers_ptr: &'s mut Vec<ExprRef<'s>> = unsafe { &mut *&raw mut numbers };

    let mut symbol_table = SymbolTable::new(&self.assign_allocator, eval_allocator, globals_ptr, numbers_ptr);
    let results = self
      .program_parser
      .parse(&mut symbol_table, code)
      .map_err(|e| format!("{name_str}parsing error: {e}"))?;

    symbol_table.print_messages();
    if symbol_table.has_errors() {
      return Err(format!("{name_str}failed to load code").into());
    }

    // TODO: evaluate the expressions somehow

    Ok(results)
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
    // Safety: the compiler isn't smart enough to know that this borrow doesn't need to outlive the function
    let mut globals = self.globals.borrow_mut();
    let globals_ptr: &'s mut HashMap<&'s str, ExprRef<'s>> = unsafe { &mut *&raw mut globals };

    let mut numbers = self.numbers.borrow_mut();
    let numbers_ptr: &'s mut Vec<ExprRef<'s>> = unsafe { &mut *&raw mut numbers };

    let mut symbol_table = SymbolTable::new(&self.assign_allocator, eval_allocator, globals_ptr, numbers_ptr);
    let result = self
      .statement_parser
      .parse(&mut symbol_table, code)
      .map_err(|e| format!("parsing error: {e}"))?;

    symbol_table.print_messages();
    if symbol_table.has_errors() {
      return Err(format!("failed to evaluate statement").into());
    }

    // TODO: evaluate the expressions somehow

    Ok(result)
  }
}
