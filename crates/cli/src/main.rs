use clap::Parser;

/// Flow Buddy: improve all the time without stopping to study.
#[derive(Parser)]
#[command(name = "flowbuddy", version = flowbuddy_core::VERSION)]
struct Args {}

fn main() {
    let _args = Args::parse();
    println!(
        "flowbuddy {} (not implemented yet)",
        flowbuddy_core::VERSION
    );
}
