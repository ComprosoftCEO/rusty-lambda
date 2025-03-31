use lalrpop_util::lalrpop_mod;
use std::num::NonZero;

pub mod expr;
pub mod symbol_table;

lalrpop_mod!(pub lambda);

fn main() {
  let allocator = expr::Allocator::new();

  let lambda = allocator.new_lambda(
    "x",
    allocator.new_lambda(
      "x",
      allocator.new_eval(
        allocator.new_term(NonZero::new(1).unwrap()),
        allocator.new_term(NonZero::new(2).unwrap()),
      ),
    ),
  );

  println!("{}", lambda)
}
