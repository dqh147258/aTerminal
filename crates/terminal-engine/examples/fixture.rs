use ai_terminal_engine::Engine;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os().nth(1).ok_or("expected output path")?;
    let mut e = Engine::new(12, 48, 1)?;
    e.feed("AI Terminal\r\n\r\n同一会话，同一屏幕。\r\n\x1b[32mready\x1b[0m  \x1b[1mbold\x1b[0m  e\u{301}\r\n\r\n$ ".as_bytes());
    let state = e.snapshot();
    state.validate()?;
    std::fs::write(path, state.wire())?;
    Ok(())
}
