fn main() {
    if let Err(error) = film_fx::run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
