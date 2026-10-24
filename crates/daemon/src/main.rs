use clap::Parser;

/// Flow Buddy daemon: watches how you work and coaches you locally.
#[derive(Parser)]
#[command(name = "flowbuddyd", version = flowbuddy_core::VERSION)]
struct Args {}

fn main() {
    let _args = Args::parse();
    println!(
        "flowbuddyd {} (not implemented yet)",
        flowbuddy_core::VERSION
    );
}
