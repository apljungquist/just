use sentry::ClientInitGuard;
use std::path::Path;

fn repository_root(start: &Path) -> Option<&Path> {
  for path in start.ancestors() {
    if path.join(".git").exists() {
      return Some(path);
    }
  }
  None
}

pub fn path_from_repository_root(path: &Path) -> Option<&Path> {
  match repository_root(path) {
    Some(p) => Some(path.strip_prefix(p).unwrap()),
    None => None,
  }
}

#[must_use]
pub fn guard() -> Option<ClientInitGuard> {
  odx::dsn().ok().filter(|dsn| !dsn.is_empty()).map(|dsn| {
    sentry::init((
      dsn,
      sentry::ClientOptions {
        release: sentry::release_name!(),
        traces_sample_rate: 1.0,
        ..Default::default()
      },
    ))
  })
}

pub struct Transaction {
  transaction: sentry::Transaction,
  name: String,
}

impl Transaction {
  pub fn new(name: String) -> Self {
    let ctx = sentry::TransactionContext::new(&name, "ui.action");
    let transaction = sentry::start_transaction(ctx);

    sentry::configure_scope(|scope| {
      scope.set_span(Some(sentry::TransactionOrSpan::Transaction(
        transaction.clone(),
      )));

      #[cfg(unix)]
      if let Ok(username) = std::env::var("USER") {
        scope.set_user(Some(sentry::User {
          username: Some(username),
          ..Default::default()
        }));
      }
    });

    sentry::capture_message("Sentry initialized", sentry::Level::Info);
    Self { transaction, name }
  }

  pub fn fail(self, code: Option<i32>) {
    let Self { transaction, name } = self;
    let msg = match code {
      None => format!("{name} failed"),
      Some(code) => format!("{name} failed ({code})"),
    };
    sentry::capture_message(&msg, sentry::Level::Warning);
    transaction.set_status(sentry::protocol::SpanStatus::UnknownError);
    transaction.finish();
  }

  pub fn pass(self) {
    let Self { transaction, name } = self;
    transaction.set_status(sentry::protocol::SpanStatus::Ok);
    sentry::capture_message(&format!("{name} succeeded"), sentry::Level::Info);
    transaction.finish();
  }
}
