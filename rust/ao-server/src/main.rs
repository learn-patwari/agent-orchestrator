fn main() {
    // The axum server arrives in phase 7 of docs/design/rust-port.md.
    println!("ao-backend {}", env!("CARGO_PKG_VERSION"));
}
