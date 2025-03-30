mod expr;
mod term;

fn main() {
  println!("Hello, world! {}", size_of::<term::Term>());
}
