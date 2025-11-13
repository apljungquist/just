use crate::color::Color;
use crate::color_display::ColorDisplay;
use crate::error::Error;
use log::{error, warn};
use sentry::ClientInitGuard;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

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

fn git_describe() -> Option<String> {
  let output = Command::new("git")
    .args(["describe", "--dirty", "--tags"])
    .output()
    .inspect_err(|e| warn!("Could not get git commit: {e:?}"))
    .ok()?;
  String::from_utf8(output.stdout)
    .inspect_err(|e| warn!("Could not parse git commit: {e:?}"))
    .ok()
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
  transaction: Option<sentry::Transaction>,
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

      if let Ok(shell) = std::env::var("SHELL") {
        match PathBuf::from(shell).file_name() {
          None => error!("SHELL does not have a file name"),
          Some(name) => scope.set_tag("shell", name.to_string_lossy()),
        }
      }

      if let Some(commit) = git_describe() {
        scope.set_extra("commit", Value::String(commit));
      }
    });

    sentry::capture_message("Sentry initialized", sentry::Level::Info);
    sentry::capture_message(&format!("{name} started"), sentry::Level::Info);
    Self {
      transaction: Some(transaction),
      name,
    }
  }

  pub fn fail(mut self, e: &Error) {
    let Self { transaction, name } = &mut self;
    let transaction = transaction.take().unwrap();

    sentry::capture_message(
      e.color_display(Color::never()).to_string().as_str(),
      sentry::Level::Info,
    );
    match e {
      Error::Signal { .. } => {
        sentry::capture_message(&format!("{name} terminated"), sentry::Level::Warning);
        transaction.set_status(sentry::protocol::SpanStatus::Aborted);
      }
      e => {
        let msg = match e.code() {
          None => format!("{name} failed"),
          Some(code) => format!("{name} failed ({code})"),
        };
        sentry::capture_message(&msg, sentry::Level::Warning);
        transaction.set_status(sentry::protocol::SpanStatus::UnknownError);
      }
    }
    transaction.finish();
  }

  pub fn pass(mut self) {
    let Self { transaction, name } = &mut self;
    let transaction = transaction.take().unwrap();
    transaction.set_status(sentry::protocol::SpanStatus::Ok);
    // TODO: Consider removing; can be inferred.
    sentry::capture_message(&format!("{name} succeeded"), sentry::Level::Info);
    transaction.finish();
  }
}

impl Drop for Transaction {
  fn drop(&mut self) {
    if let Some(transaction) = self.transaction.take() {
      sentry::capture_message("Transaction was never consumed", sentry::Level::Error);
      transaction.finish();
    }
  }
}
