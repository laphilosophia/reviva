fn main() {
    if let Err(error) = reviva::run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
