use walkdir::WalkDir;

fn main() {
    println!("cargo:rerun-if-changed=webapp/dist/server_stats");
    for entry in WalkDir::new("webapp/dist/server_stats")
        .into_iter()
        .flatten()
    {
        println!("cargo:rerun-if-changed={}", entry.path().display());
    }
}
