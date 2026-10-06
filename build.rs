// Re-embed migrations in `sqlx::migrate!` whenever the directory changes.
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}
