use std::env::VarError;

pub fn dsn() -> Result<String, VarError> {
  match cfg!(debug_assertions) {
    true => std::env::var("SANDBOX_DSN"),
    false => std::env::var("ODX_DSN"),
  }
}
