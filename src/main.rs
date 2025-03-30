use std::num::NonZero;

mod expr;

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
