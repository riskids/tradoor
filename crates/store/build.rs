// The migrations are embedded at compile time; a new file must trigger a rebuild.
fn main() {
    println!("cargo:rerun-if-changed=../../migrations");
}
