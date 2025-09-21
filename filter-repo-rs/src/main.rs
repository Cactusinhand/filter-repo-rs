use filter_repo_rs as fr;
use std::io;

fn main() -> io::Result<()> {
    let opts = fr::opts::parse_args();
    fr::run(&opts)
}
