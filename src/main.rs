extern crate pest;
#[macro_use]
extern crate pest_derive;

mod codegen;
mod config;
mod parser;

use clap::Parser;
use log::{error, info};
use simple_logger::SimpleLogger;
use std::fs;

const ERR_CODE_CONFIG_READ: u32 = 1;
const ERR_CODE_CONFIG_LOAD: u32 = 2;

#[derive(Parser, Debug)]
#[clap(name = "Breadlog")]
#[clap(author = "James Mistry")]
#[clap(about = "Maintain unique references to log messages in source code.", long_about = None)]
struct ProgArgs
{
    #[clap(short, long, value_parser)]
    /// YAML configuration file. Its format is described in detail at https://example.com/docs
    config: String,

    #[clap(long, default_value_t = false)]
    /// Check all log messages have valid references, but don't modify any code. If the check fails, exits with a non-zero code.
    check: bool,
}

fn setup_context(config_filename: &String, check_mode: bool) -> Result<config::Context, u32>
{
    let config_contents = fs::read_to_string(&config_filename);

    info!("Reading configuration file: {}", config_filename);

    let app_ctx = match config_contents
    {
        Err(e) =>
        {
            error!("Failed to read configuration file: {}", e.to_string());
            return Err(ERR_CODE_CONFIG_READ);
        },

        Ok(yaml) => config::Context::new(yaml, check_mode),
    };

    match app_ctx
    {
        Err(e) =>
        {
            error!("Failed to load YAML: {}", e);
            Err(ERR_CODE_CONFIG_LOAD)
        },

        Ok(ctx) =>
        {
            info!("Configuration loaded!");
            Ok(ctx)
        },
    }
}

fn main() -> Result<(), u32>
{
    use std::sync::Arc;

    const INIT_ERR_CODE: u32 = 1;
    const CODE_GEN_ERR_CODE: u32 = 2;

    SimpleLogger::new().init().unwrap();

    let args = ProgArgs::parse();

    let app_context_parsed = setup_context(&args.config, args.check);

    let app_context = match app_context_parsed
    {
        Err(e) => return Err(INIT_ERR_CODE),
        Ok(c) => c,
    };

    /*
     * Set up the signal handler.
     */
    if signal_hook::flag::register(
        signal_hook::consts::SIGTERM | signal_hook::consts::SIGINT,
        Arc::clone(&app_context.stop_commanded),
    )
    .is_err()
    {
        error!("Failed to register signal handler");
        return Err(INIT_ERR_CODE);
    }

    if let Err(err) = codegen::generate::generate_code(&app_context)
    {
        error!("Failed or incomplete code generation: {}", err);
        return Err(CODE_GEN_ERR_CODE);
    }

    Ok(())
}

#[cfg(test)]
mod tests
{
    use super::*;
    use std::fs;
    use std::os::unix::prelude::PermissionsExt;
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[test]
    fn test_context_load_file_error()
    {
        let scratch_dir = TempDir::new().unwrap();
        let scratch_path = scratch_dir.path();
        let invalid_file_path: PathBuf = scratch_path.join("non_existent.yaml");
        let invalid_file_path_string = String::from(invalid_file_path.to_str().unwrap());

        assert_eq!(invalid_file_path.exists(), false);
        assert!(setup_context(&invalid_file_path_string, false).is_err());
    }

    #[test]
    fn test_context_load_yaml_error()
    {
        let scratch_dir = TempDir::new().unwrap();
        let scratch_path = scratch_dir.path();
        let config_file_path: PathBuf = scratch_path.join("invalid_yaml.yaml");
        let config_file_path_string = String::from(config_file_path.to_str().unwrap());

        fs::metadata(scratch_path)
            .unwrap()
            .permissions()
            .set_mode(0o770);
        fs::write(
            config_file_path_string.as_str(),
            "---\n: this is invalid YAML\n  -",
        )
        .unwrap();

        assert_eq!(config_file_path.exists(), true);
        assert!(setup_context(&config_file_path_string, false).is_err());
    }
}
