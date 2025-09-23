use filter_repo_rs as fr;
use std::error::Error;
use std::process;

fn main() {
    let opts = fr::opts::parse_args();
    if let Err(err) = fr::run(&opts) {
        eprintln!("{err}");
        let mut source = err.source();
        while let Some(cause) = source {
            eprintln!("Caused by: {cause}");
            source = cause.source();
        }
        process::exit(1);
    }
}
