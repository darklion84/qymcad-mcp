fn main() {
    println!(
        "qymcad-mcp {} (QymCAD {})",
        env!("CARGO_PKG_VERSION"),
        qymcad_engine::QYMCAD_VERSION
    );
}
