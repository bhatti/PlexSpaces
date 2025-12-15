// Build script is not needed since we have a separate proto crate
// that handles all proto generation
fn main() {
    // Proto generation is handled by crates/proto/build.rs
    println!("cargo:rerun-if-changed=build.rs");
}
