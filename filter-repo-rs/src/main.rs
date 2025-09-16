mod message;
mod pathutil;
mod gitutil;
mod opts;
mod pipes;
mod tag;
mod commit;
mod filechange;
mod finalize;
mod migrate;
mod stream;

use std::io;

fn main() -> io::Result<()> {
  let opts = crate::opts::parse_args();
  crate::stream::run(&opts)
}
