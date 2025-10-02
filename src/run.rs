use super::*;

/// Main entry point into `just`. Parse arguments from `args` and run.
#[allow(clippy::missing_errors_doc)]
pub fn run(args: impl Iterator<Item = impl Into<OsString> + Clone>) -> Result<(), i32> {
  #[cfg(windows)]
  ansi_term::enable_ansi_support().ok();

  // Initialize Sentry if DX_DSN is set
  let _sentry_guard = std::env::var("DX_DSN")
    .ok()
    .filter(|dsn| !dsn.is_empty())
    .map(|dsn| {
      sentry::init((
        dsn,
        sentry::ClientOptions {
          release: sentry::release_name!(),
          traces_sample_rate: 1.0,
          ..Default::default()
        },
      ))
    });

  let app = Config::app();

  let matches = app.try_get_matches_from(args).map_err(|err| {
    err.print().ok();
    err.exit_code()
  })?;

  let config = Config::from_matches(&matches).map_err(Error::from);

  let (color, verbosity, subcommand_name) = config
    .as_ref()
    .map(|config| (config.color, config.verbosity, config.subcommand.name()))
    .unwrap_or_default();

  // Start a transaction/span if Sentry is enabled
  let ctx = sentry::TransactionContext::new(&subcommand_name, "ui.action");
  let transaction = sentry::start_transaction(ctx);

  // Bind transaction to current scope and set user
  sentry::configure_scope(|scope| {
    scope.set_span(Some(sentry::TransactionOrSpan::Transaction(
      transaction.clone(),
    )));

    // Set username on Unix systems (macOS and Linux)
    #[cfg(unix)]
    if let Ok(username) = std::env::var("USER") {
      scope.set_user(Some(sentry::User {
        username: Some(username),
        ..Default::default()
      }));
    }
  });

  let loader = Loader::new();

  let result = config
    .and_then(|config| {
      SignalHandler::install(config.verbosity)?;
      config.subcommand.execute(&config, &loader)
    })
    .map_err(|error| {
      if !verbosity.quiet() && error.print_message() {
        eprintln!("{}", error.color_display(color.stderr()));
      }
      error.code().unwrap_or(EXIT_FAILURE)
    });

  // Log warning with trace ID if command failed
  if let Err(code) = result {
    sentry::capture_message(
      &format!(
        "Command failed with exit code {}: {}",
        code, subcommand_name
      ),
      sentry::Level::Warning,
    );
    transaction.set_status(sentry::protocol::SpanStatus::UnknownError);
    transaction.finish();
    // Flush events to ensure they're sent before exit
    if let Some(client) = sentry::Hub::current().client() {
      client.close(Some(std::time::Duration::from_secs(2)));
    }
    return Err(code);
  }

  transaction.set_status(sentry::protocol::SpanStatus::Ok);
  sentry::capture_message(
    &format!("Command succeeded: {}", subcommand_name),
    sentry::Level::Info,
  );
  transaction.finish();

  // Flush events to ensure they're sent before exit
  if let Some(client) = sentry::Hub::current().client() {
    client.close(Some(std::time::Duration::from_secs(2)));
  }

  Ok(())
}
