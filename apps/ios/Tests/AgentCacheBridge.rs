//! Host regression adapter using the actual bounded Rust AgentCache implementation.
use ai_terminal_mobile::AgentCache;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let cache = AgentCache::open(args[1].clone())?;
    match args[2].as_str() {
        "open" => {}
        "put" => cache.store_page(args[3].clone(), (!args[4].is_empty()).then(|| args[4].clone()), std::fs::read_to_string(&args[5])?)?,
        "get" => {
            if let Some(page) = cache.page(args[3].clone(), (!args[4].is_empty()).then(|| args[4].clone()))? { print!("{page}"); }
        }
        "reconcile" => cache.reconcile(args[3].clone(), args[4].parse()?)?,
        _ => return Err("unknown cache operation".into()),
    }
    Ok(())
}
