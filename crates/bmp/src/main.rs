fn main() {
    if std::env::args().any(|a| a == "--version") {
        println!("bmp {}", env!("CARGO_PKG_VERSION"));
    }
}
