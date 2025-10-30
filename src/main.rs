fn main() {
  if let Err(code) = jt::run(std::env::args_os()) {
    std::process::exit(code);
  }
}
