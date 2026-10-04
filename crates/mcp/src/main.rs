use qymcad_mcp::{app_version_note, instructions, tools::Registry, transport::Server};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--dump-tools") => print!("{}", Registry::new().markdown()),
        Some("--version") => println!("qymcad-mcp {} (QymCAD {})", env!("CARGO_PKG_VERSION"), qymcad_engine::QYMCAD_VERSION),
        Some(other) => {
            eprintln!("unknown argument {other}; usage: qymcad-mcp [--dump-tools | --version]   (no argument: serve MCP on stdio)");
            std::process::exit(2);
        }
        None => {
            let note = app_version_note();
            eprintln!("qymcad-mcp {} (QymCAD {}): {note}", env!("CARGO_PKG_VERSION"), qymcad_engine::QYMCAD_VERSION);
            let mut server = Server::new(Registry::new(), instructions(&note));
            let stdin = std::io::stdin();
            if let Err(e) = server.run(stdin.lock(), std::io::stdout().lock()) {
                eprintln!("qymcad-mcp: i/o error: {e}");
                std::process::exit(1);
            }
        }
    }
}
