use std::{marker::PhantomData, num::NonZero, slice, str};
use typed_arena::Arena;

/// Visit a Lambda expression
pub trait ExprVisitor {
  type Output;

  fn visit_term(&mut self, de_bruijn_index: NonZero<u64>) -> Self::Output;

  fn visit_lambda<'a>(&mut self, body: ExprRef<'a>, parameter_name: &'a str) -> Self::Output;

  fn visit_eval<'a>(&mut self, left: ExprRef<'a>, right: ExprRef<'a>) -> Self::Output;
}

const LENGTH_MASK: u64 = 0xffff_0000_0000_0000;
const LENGTH_SHIFT: u64 = 48;
const MAX_LENGTH: u64 = 0xffff;
const POINTER_MASK: u64 = 0x0000_ffff_ffff_ffff;

/// Reference to a Lambda expression.
///
/// - Term 1 to 65535 = `xxxx 0000 0000 0000`
/// - Normal Pointer  = `0000 xxxx xxxx xxxx`
#[derive(Debug, Clone, Copy)]
pub struct ExprRef<'a>(u64, PhantomData<&'a CompactExpr>);

#[allow(unused)]
impl ExprRef<'_> {
  #[inline]
  pub fn visit<V: ExprVisitor>(self, visitor: &mut V) -> <V as ExprVisitor>::Output {
    if self.0 & POINTER_MASK == 0 {
      // Safety: we only construct an ExprRef from a non-zero length
      let de_bruijn_index = unsafe { NonZero::new_unchecked(self.0 >> LENGTH_SHIFT) };
      visitor.visit_term(de_bruijn_index)
    } else {
      // Safety: these are only constructed from valid references and can't
      // outlive the lifetime of the arena allocator.
      let compact_expr_ref = unsafe { &*((self.0 & POINTER_MASK) as *const CompactExpr) };
      compact_expr_ref.visit(visitor)
    }
  }
}

/// Very efficient way to represent a Lambda expression
///
/// - Term:
///   - Left  = 0
///   - Right = Index (1 to 2^64-1)
/// - Lambda
///   - Left = [ExprRef](ExprRef)
///   - Right = `&str` where `x` stores the length (1 to 65535) and `y` stores the pointer bits: `xxxx yyyy yyyy yyyy`.
/// - Eval
///   - Left = [ExprRef](ExprRef)
///   - Right = [ExprRef](ExprRef)
#[derive(Debug, Clone, Copy)]
struct CompactExpr {
  left: u64,
  right: u64,
}

#[allow(unused)]
impl CompactExpr {
  pub fn new_term(de_bruijn_index: NonZero<u64>) -> Self {
    Self {
      left: 0,
      right: de_bruijn_index.get(),
    }
  }

  pub fn new_lambda<'a>(body: ExprRef<'a>, param_name: &'a str) -> Self {
    debug_assert!(!param_name.is_empty(), "Lambda cannot have an empty parameter name");
    debug_assert!(
      param_name.len() as u64 <= MAX_LENGTH,
      "Lambda name is too long ({} > {})",
      param_name.len(),
      MAX_LENGTH
    );

    let ptr = param_name.as_bytes().as_ptr() as u64;
    debug_assert!(ptr & LENGTH_MASK == 0, "Name has pointer high bits set to 0");

    Self {
      left: body.0,
      right: ((param_name.len() as u64) << LENGTH_SHIFT) | ptr,
    }
  }

  pub fn new_eval<'a>(left: ExprRef<'a>, right: ExprRef<'a>) -> Self {
    Self {
      left: left.0,
      right: right.0,
    }
  }

  pub fn visit<V: ExprVisitor>(self, visitor: &mut V) -> <V as ExprVisitor>::Output {
    if self.left == 0 {
      let de_bruijn_index = unsafe { NonZero::new_unchecked(self.right) };
      return visitor.visit_term(de_bruijn_index);
    }

    let left = ExprRef(self.left, PhantomData);
    if self.right & POINTER_MASK == 0 || self.right & LENGTH_MASK == 0 {
      let right = ExprRef(self.right, PhantomData);
      return visitor.visit_eval(left, right);
    }

    let param_name_ptr = (self.right & POINTER_MASK) as *const u8;
    let param_name_len = (self.right >> LENGTH_SHIFT) as usize;
    let param_name = unsafe { str::from_utf8_unchecked(slice::from_raw_parts(param_name_ptr, param_name_len)) };

    visitor.visit_lambda(left, param_name)
  }
}

/// Handles allocation of Lambda expressions
pub struct Allocator {
  arena: Arena<CompactExpr>,
}

#[allow(unused)]
impl Allocator {
  pub fn new() -> Self {
    Self { arena: Arena::new() }
  }

  #[allow(clippy::needless_lifetimes)]
  pub fn new_term<'a>(&'a self, de_bruijn_index: NonZero<u64>) -> ExprRef<'a> {
    // Special case: short indexes have a very compact representation
    if de_bruijn_index.get() < MAX_LENGTH {
      return ExprRef(de_bruijn_index.get() << 48, PhantomData);
    }

    // Otherwise, do a real allocation
    let term = self.arena.alloc(CompactExpr::new_term(de_bruijn_index));
    let term_ptr = term as *const CompactExpr as u64;
    debug_assert!(term_ptr & LENGTH_MASK == 0, "Term pointer has high bits set to 0");

    ExprRef(term_ptr, PhantomData)
  }

  /// The parameter name must be 65,535 characters or less
  pub fn new_lambda<'a>(&'a self, body: ExprRef<'a>, param_name: &'a str) -> ExprRef<'a> {
    let lambda = self.arena.alloc(CompactExpr::new_lambda(body, param_name));
    let lambda_ptr = lambda as *const CompactExpr as u64;
    debug_assert!(lambda_ptr & LENGTH_MASK == 0, "Lambda pointer has high bits set to 0");

    ExprRef(lambda_ptr, PhantomData)
  }

  pub fn new_eval<'a>(&'a self, left: ExprRef<'a>, right: ExprRef<'a>) -> ExprRef<'a> {
    let eval = self.arena.alloc(CompactExpr::new_eval(left, right));
    let eval_ptr = eval as *const CompactExpr as u64;
    debug_assert!(eval_ptr & LENGTH_MASK == 0, "Eval pointer has high bits set to 0");

    ExprRef(eval_ptr, PhantomData)
  }
}
