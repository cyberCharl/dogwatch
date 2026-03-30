fn main() {
    if let Err(error) = dogwatch::run() {
        eprintln!("dogwatch: {error:#}");
        std::process::exit(1);
    }
}
