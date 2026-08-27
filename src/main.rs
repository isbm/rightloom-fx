fn main() {
    if let Err(error) = rightloom_fx::run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
