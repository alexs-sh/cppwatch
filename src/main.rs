mod event;
mod filters;
mod reporter;
mod watcher;

use clap::Parser;
use std::io::Result;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(index = 1)]
    watch_dir: String,

    #[arg(long, default_value = "")]
    build_dir: String,

    #[arg(short, long, default_value = "make -j4")]
    build_command: String,

    #[arg(short, long, default_value = "make test")]
    test_command: String,

    #[arg(short, long, default_value = "0")]
    delay: String,
}

fn read_delay(args: &Args) -> Option<Duration> {
    if args.delay.is_empty() {
        None
    } else {
        args.delay.parse::<u64>().ok().map(Duration::from_secs)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let delay = read_delay(&args);
    let (tx, rx) = event::make_channel();
    let config = watcher::Config {
        watch_dir: args.watch_dir,
        build_dir: args.build_dir,
        build_command: args.build_command,
        test_command: args.test_command,
        delay,
        tx,
    };
    let watcher = watcher::run(config)?;
    let reporter = reporter::run(rx)?;
    let _ = tokio::join!(watcher, reporter);
    Ok(())
}
