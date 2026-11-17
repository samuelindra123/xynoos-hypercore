//! Distribution entry point: observe, owned-buffer simulation and read-only recovery.
#![forbid(unsafe_code)]
#[path = "../mvp.rs"]
mod mvp;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args == ["help"] {
        println!("xynoos-mvp observe [--count N] [--interval-ms N] [--human]\nxynoos-mvp simulate [--trace FILE] [--journal NEW_FILE] [--human]\nxynoos-mvp recovery [--journal FILE] [--human]");
        return Ok(());
    }
    if !matches!(
        args.first().map(String::as_str),
        None | Some("help" | "observe" | "simulate" | "recovery")
    ) {
        return Err("distribution permits only observe, simulate and read-only recovery".into());
    }
    mvp::run(&args)
}
