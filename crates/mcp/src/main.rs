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
            let out = match protocol_stdout() {
                Ok(f) => std::io::BufWriter::new(f),
                Err(e) => {
                    eprintln!("qymcad-mcp: cannot set up stdout: {e}");
                    std::process::exit(1);
                }
            };
            let stdin = std::io::stdin();
            if let Err(e) = server.run(stdin.lock(), out) {
                eprintln!("qymcad-mcp: i/o error: {e}");
                std::process::exit(1);
            }
        }
    }
}

/// The protocol channel, moved off file descriptor 1 (ADR 0005, FINDINGS F-3C-1).
///
/// OpenCASCADE prints to the process's stdout (for example "Statistics on Transfer (Write)" on every STEP write);
/// on stdio that text lands in the JSON-RPC stream and breaks the client. So the protocol gets its own duplicate of
/// the original stdout, and fd 1 is pointed at stderr, where whatever the kernel prints is harmless.
fn protocol_stdout() -> std::io::Result<std::fs::File> {
    use std::os::fd::AsFd;
    let proto = std::io::stdout().as_fd().try_clone_to_owned()?;
    redirect_fd1_to_stderr()?;
    Ok(std::fs::File::from(proto))
}

#[allow(unsafe_code)]
fn redirect_fd1_to_stderr() -> std::io::Result<()> {
    // SAFETY: `dup2` takes plain descriptor numbers and touches no Rust memory. fds 1 and 2 stay open for the whole
    // process; afterwards fd 1 is a duplicate of fd 2, so `std::io::stdout()` and C++ `std::cout` write to stderr.
    let rc = unsafe { libc::dup2(libc::STDERR_FILENO, libc::STDOUT_FILENO) };
    if rc < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
