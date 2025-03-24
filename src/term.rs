use std::cell::RefCell;
use std::fmt;

pub type TermRef<'a> = &'a RefCell<Term<'a>>;

#[derive(Debug, Clone)]
pub enum Term<'a> {
  // De Bruijn index
  Variable(u64),
  // \x.Body
  Lambda(TermRef<'a>),
  // (f x)
  Eval(TermRef<'a>, TermRef<'a>),
}

impl fmt::Display for Term<'_> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    use Term::*;

    fn term_to_string(term: &Term<'_>, level: u64) -> String {
      match term {
        Variable(index) => index_to_variable_name(level - *index - 1),
        Lambda(func) => format!(
          "(λ{}.{})",
          index_to_variable_name(level),
          term_to_string(&func.borrow(), level + 1)
        ),
        Eval(left, right) => format!(
          "({} {})",
          term_to_string(&left.borrow(), level + 1),
          term_to_string(&right.borrow(), level + 1)
        ),
      }
    }

    write!(f, "{}", term_to_string(self, 0))
  }
}

fn index_to_variable_name(mut input: u64) -> String {
  static ALL_VARIABLES: &[char] = &[
    'x', 'y', 'z', 'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's', 't',
    'u', 'v', 'w',
  ];
  const NUM_VARIABLES: usize = ALL_VARIABLES.len();

  let mut result = String::new();
  while (input as usize) > NUM_VARIABLES {
    result.push(ALL_VARIABLES[(input as usize) % NUM_VARIABLES]);
    input /= NUM_VARIABLES as u64;
  }

  result.chars().rev().collect()
}
