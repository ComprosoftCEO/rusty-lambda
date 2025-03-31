use clap::{Parser, Subcommand};
use lalrpop_util::lalrpop_mod;

pub mod command;
pub mod expr;
pub mod symbol_table;

lalrpop_mod!(pub lambda);

pub static PRELUDE: &str = include_str!("prelude.txt");

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
struct Opt {
  #[clap(flatten)]
  run_args: command::RunArgs,

  #[clap(subcommand)]
  subcommand: Option<SubCommand>,
}

#[derive(Subcommand)]
enum SubCommand {
  Encode(command::EncodeArgs),
}

fn main() -> command::CommandResult {
  let opt = Opt::parse();
  match opt.subcommand {
    None => opt.run_args.execute(),
    Some(command) => {
      use SubCommand::*;
      match command {
        Encode(args) => args.execute(),
      }
    },
  }
}
