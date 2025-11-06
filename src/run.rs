use super::*;

/// Main entry point into `just`. Parse arguments from `args` and run.
#[allow(clippy::missing_errors_doc)]
pub fn run(args: impl Iterator<Item = impl Into<OsString> + Clone>) -> Result<(), i32> {
  #[cfg(windows)]
  ansi_term::enable_ansi_support().ok();

  let app = Config::app();

  let matches = app.try_get_matches_from(args).map_err(|err| {
    err.print().ok();
    err.exit_code()
  })?;

  let config = Config::from_matches(&matches).map_err(Error::from);

  let (color, verbosity, path, subcommand_name) = config
    .as_ref()
    .map(|config| {
      (
        config.color,
        config.verbosity,
        match &config.search_config {
          SearchConfig::FromInvocationDirectory => config.invocation_directory.clone(),
          SearchConfig::FromSearchDirectory { search_directory } => search_directory.clone(),
          SearchConfig::GlobalJustfile => Default::default(),
          SearchConfig::WithJustfile { justfile } => justfile.clone(),
          SearchConfig::WithJustfileAndWorkingDirectory { justfile, .. } => justfile.clone(),
        },
        config.subcommand.name(),
      )
    })
    .unwrap_or_default();

  // Truncate path to make it independent of where the repository is located.
  // This will be unambiguous as long as directories with `justfile`s have unique names.
  // TODO: Consider getting path relative to repository root
  let path = path.file_name().unwrap_or_default().to_string_lossy();
  let path_and_command = format!("{subcommand_name} ({path})");
  // Start a transaction/span if Sentry is enabled

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
    // Flush events to ensure they're sent before exit
    if let Some(client) = sentry::Hub::current().client() {
      client.close(Some(std::time::Duration::from_secs(2)));
    }
    return Err(code);
  }

  // Flush events to ensure they're sent before exit
  if let Some(client) = sentry::Hub::current().client() {
    client.close(Some(std::time::Duration::from_secs(2)));
  }

  Ok(())
}
