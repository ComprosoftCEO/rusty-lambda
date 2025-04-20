use std::{collections::HashMap, fmt, marker::PhantomData, num::NonZero, slice, str};
use typed_arena::Arena;

/// Visit a Lambda expression
pub trait ExprVisitor<'a> {
  type Output;

  fn visit_term(&mut self, expr: ExprRef<'a>, de_bruijn_index: NonZero<u64>) -> Self::Output;

  fn visit_lambda(&mut self, expr: ExprRef<'a>, body: ExprRef<'a>, parameter_name: &'a str) -> Self::Output;

  fn visit_eval(&mut self, expr: ExprRef<'a>, left: ExprRef<'a>, right: ExprRef<'a>) -> Self::Output;
}

const IS_TERM_BIT: u64 = 0x8000_0000_0000_0000;
const TERM_MASK: u64 = 0x7fff_ffff_ffff_ffff;
const POINTER_MASK: u64 = 0x0000_ffff_ffff_ffff;

const STR_LENGTH_MASK: u64 = 0xffff_0000_0000_0000;
const STR_LENGTH_SHIFT: u64 = 48;
const MAX_STR_LENGTH: u64 = 0x7fff;

/// Reference to a Lambda expression.
///
/// - Term     = `1xxx xxxx xxxx xxxx`
/// - Pointer  = `0000 xxxx xxxx xxxx`
///
/// Two ExprRefs are considered equal if they point to the same object in memory,
/// not necessarily that they are isomorphic to each other. (reference equality)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExprRef<'a>(NonZero<u64>, PhantomData<&'a CompactExpr>);

pub enum UnpackedExpr<'a> {
  Term { de_bruijn_index: NonZero<u64> },
  Lambda { parameter_name: &'a str, body: ExprRef<'a> },
  Eval { left: ExprRef<'a>, right: ExprRef<'a> },
}

impl<'a> ExprRef<'a> {
  #[inline]
  pub fn visit<V: ExprVisitor<'a>>(self, visitor: &mut V) -> <V as ExprVisitor<'a>>::Output {
    if (self.0.get() & IS_TERM_BIT) != 0 {
      // Safety: we only construct an ExprRef from a non-zero length
      let de_bruijn_index = unsafe { NonZero::new_unchecked(self.0.get() & TERM_MASK) };
      visitor.visit_term(self, de_bruijn_index)
    } else {
      // Safety: these are only constructed from valid references and can't
      // outlive the lifetime of the arena allocator.
      let compact_expr_ref = unsafe { &*((self.0.get() & POINTER_MASK) as *const CompactExpr) };
      compact_expr_ref.visit(self, visitor)
    }
  }

  pub fn unpack(self) -> UnpackedExpr<'a> {
    struct UnpackVisitor;

    impl<'a> ExprVisitor<'a> for UnpackVisitor {
      type Output = UnpackedExpr<'a>;

      fn visit_term(&mut self, _: ExprRef<'a>, de_bruijn_index: NonZero<u64>) -> Self::Output {
        UnpackedExpr::Term { de_bruijn_index }
      }

      fn visit_lambda(&mut self, _: ExprRef<'a>, body: ExprRef<'a>, parameter_name: &'a str) -> Self::Output {
        UnpackedExpr::Lambda { parameter_name, body }
      }

      fn visit_eval(&mut self, _: ExprRef<'a>, left: ExprRef<'a>, right: ExprRef<'a>) -> Self::Output {
        UnpackedExpr::Eval { left, right }
      }
    }

    self.visit(&mut UnpackVisitor)
  }
}

impl fmt::Display for ExprRef<'_> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    struct Visitor<'f, 'ff, 's> {
      f: &'f mut fmt::Formatter<'ff>,
      lambda_parameters: Vec<(&'s str, u64)>,
      shadowed_variables: HashMap<&'s str, u64>,
    }

    impl<'s> ExprVisitor<'s> for Visitor<'_, '_, 's> {
      type Output = fmt::Result;

      fn visit_term(&mut self, _: ExprRef<'s>, de_bruijn_index: NonZero<u64>) -> Self::Output {
        if self.f.sign_plus() {
          write!(self.f, "{}", de_bruijn_index)?;
        } else if self.f.sign_minus() {
          write!(self.f, "-{}", de_bruijn_index)?;
        } else {
          // Read the tern name from the vector of parameters
          let term = self
            .lambda_parameters
            .get(self.lambda_parameters.len() - de_bruijn_index.get() as usize);

          match term {
            Some(term) => {
              write!(self.f, "{}", term.0)?;

              // Shadowed parameters
              for _ in 0..term.1 {
                write!(self.f, "′")?;
              }
            },
            // Default print the de Bruijn index to avoid a crash
            None => write!(self.f, "{}", de_bruijn_index)?,
          }
        }

        Ok(())
      }

      fn visit_lambda(&mut self, _: ExprRef<'s>, body: ExprRef<'s>, parameter_name: &'s str) -> Self::Output {
        let count = self
          .shadowed_variables
          .entry(parameter_name)
          .and_modify(|c| *c += 1)
          .or_insert(0);

        if self.f.alternate() {
          write!(self.f, "λ{}", parameter_name)?;
        } else {
          write!(self.f, "\\{}", parameter_name)?;
        }

        for _ in 0..*count {
          write!(self.f, "′")?;
        }
        write!(self.f, ".")?;

        self.lambda_parameters.push((parameter_name, *count));
        body.visit(self)?;
        self.lambda_parameters.pop();

        let result = self
          .shadowed_variables
          .entry(parameter_name)
          .and_modify(|c| {
            if *c > 0 {
              *c -= 1
            }
          })
          .or_default();
        if *result == 0 {
          self.shadowed_variables.remove(parameter_name);
        }

        Ok(())
      }

      fn visit_eval(&mut self, _: ExprRef<'s>, left: ExprRef<'s>, right: ExprRef<'s>) -> Self::Output {
        write!(self.f, "(")?;
        left.visit(self)?;
        write!(self.f, " ")?;
        right.visit(self)?;
        write!(self.f, ")")
      }
    }

    self.visit(&mut Visitor {
      f,
      lambda_parameters: Vec::new(),
      shadowed_variables: HashMap::new(),
    })
  }
}

/// Very efficient way to represent a Lambda expression
///
/// - Term - Not needed, encoded into ExprRef
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

impl CompactExpr {
  pub fn new_lambda<'a>(param_name: &'a str, body: ExprRef<'a>) -> Self {
    debug_assert!(!param_name.is_empty(), "Lambda cannot have an empty parameter name");
    debug_assert!(
      param_name.len() as u64 <= MAX_STR_LENGTH,
      "Lambda name is too long ({} > {})",
      param_name.len(),
      MAX_STR_LENGTH
    );

    let ptr = param_name.as_bytes().as_ptr() as u64;
    debug_assert!(ptr & STR_LENGTH_MASK == 0, "Name has pointer high bits set to 0");

    Self {
      left: body.0.get(),
      right: ((param_name.len() as u64) << STR_LENGTH_SHIFT) | ptr,
    }
  }

  pub fn new_eval<'a>(left: ExprRef<'a>, right: ExprRef<'a>) -> Self {
    Self {
      left: left.0.get(),
      right: right.0.get(),
    }
  }

  pub fn visit<'a, V: ExprVisitor<'a>>(self, expr: ExprRef<'a>, visitor: &mut V) -> <V as ExprVisitor<'a>>::Output {
    // Safety: we ensure you can't make a 0 reference
    let left = ExprRef(unsafe { NonZero::new_unchecked(self.left) }, PhantomData);

    if (self.right & IS_TERM_BIT != 0) || (self.right & STR_LENGTH_MASK == 0) {
      // Safety: we ensure you can't make a 0 reference
      let right = ExprRef(unsafe { NonZero::new_unchecked(self.right) }, PhantomData);
      return visitor.visit_eval(expr, left, right);
    }

    let param_name_ptr = (self.right & POINTER_MASK) as *const u8;
    let param_name_len = (self.right >> STR_LENGTH_SHIFT) as usize;
    let param_name = unsafe { str::from_utf8_unchecked(slice::from_raw_parts(param_name_ptr, param_name_len)) };

    visitor.visit_lambda(expr, left, param_name)
  }
}

/// Handles allocation of Lambda expressions
#[derive(Default)]
pub struct Allocator {
  arena: Arena<CompactExpr>,
}

impl Allocator {
  pub fn new() -> Self {
    Self { arena: Arena::new() }
  }

  #[allow(clippy::needless_lifetimes)]
  pub fn new_term<'a>(&'a self, de_bruijn_index: NonZero<u64>) -> ExprRef<'a> {
    debug_assert!(de_bruijn_index.get() <= TERM_MASK, "Term index is too large");

    // Safety: we're always setting the highest bit, which will never be 0
    let term = unsafe { NonZero::new_unchecked(de_bruijn_index.get() | IS_TERM_BIT) };
    ExprRef(term, PhantomData)
  }

  /// The parameter name must be 32,767 characters or less
  pub fn new_lambda<'a>(&'a self, param_name: &'a str, body: ExprRef<'a>) -> ExprRef<'a> {
    let lambda = self.arena.alloc(CompactExpr::new_lambda(param_name, body));
    let lambda_ptr = lambda as *const CompactExpr as u64;
    debug_assert!(
      lambda_ptr & STR_LENGTH_MASK == 0,
      "Lambda pointer has high bits set to 0"
    );

    // Safety: newly allocated pointer is never 0
    ExprRef(unsafe { NonZero::new_unchecked(lambda_ptr) }, PhantomData)
  }

  pub fn new_eval<'a>(&'a self, left: ExprRef<'a>, right: ExprRef<'a>) -> ExprRef<'a> {
    let eval = self.arena.alloc(CompactExpr::new_eval(left, right));
    let eval_ptr = eval as *const CompactExpr as u64;
    debug_assert!(eval_ptr & STR_LENGTH_MASK == 0, "Eval pointer has high bits set to 0");

    // Safety: newly allocated pointer is never 0
    ExprRef(unsafe { NonZero::new_unchecked(eval_ptr) }, PhantomData)
  }
}
