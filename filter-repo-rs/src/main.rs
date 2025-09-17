mod message;
mod pathutil;
mod gitutil;
mod backup;
mod opts;
mod pipes;
mod tag;
mod commit;
mod filechange;
mod finalize;
mod migrate;
mod stream;

use filter_repo_rs as fr;
use std::io;

fn main() -> io::Result<()> {
  let opts = fr::opts::parse_args();
  fr::run(&opts)
}
