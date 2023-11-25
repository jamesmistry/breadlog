extern crate pest;
#[macro_use]
extern crate pest_derive;

mod codegen;
mod config;
mod parser;

use clap::Parser;
use log::{error, info, LevelFilter};
use simple_logger::SimpleLogger;
use std::fs;

const ERR_CODE_CONFIG_READ: u32 = 1;
const ERR_CODE_CONFIG_LOAD: u32 = 2;

/// Command-line arguments for the program.
#[derive(Parser, Debug)]
#[clap(name = "Breadlog")]
#[clap(author = "James Mistry")]
#[clap(about = "Maintain unique references to log messages in source code.", long_about = None)]
struct ProgArgs
{
    #[clap(short, long, value_parser)]
    /// YAML configuration file. Its format is described in detail at https://example.com/docs
    config: String,

    #[clap(long, action)]
    /// Check all log messages have valid references, but don't modify any code. If the check fails, exits with a non-zero code.
    check: bool,
}

/// Set up and return the application context. This includes reading the configuration file and parsing it.
///
/// # Arguments
///
/// * `config_filename` - The path to the configuration file.
/// * `check_mode` - Whether to run in check mode or not.
///
fn setup_context(config_filename: &String, check_mode: bool) -> Result<config::Context, u32>
{
    let config_contents = fs::read_to_string(config_filename);

    info!("[ref: 22] Reading configuration file: {}", config_filename);

    let app_ctx = match config_contents
    {
        Err(e) =>
        {
            error!(
                "[ref: 23] Failed to read configuration file: {}",
                e.to_string()
            );
            return Err(ERR_CODE_CONFIG_READ);
        },

        Ok(yaml) =>
        {
            let config_dir = match std::path::Path::new(config_filename).parent()
            {
                None => String::from(""),
                Some(p) => String::from(p.to_str().unwrap()),
            };

            config::Context::new(yaml, &config_dir, check_mode)
        },
    };

    match app_ctx
    {
        Err(e) =>
        {
            error!("[ref: 24] Failed to load YAML: {}", e);
            Err(ERR_CODE_CONFIG_LOAD)
        },

        Ok(ctx) =>
        {
            info!("[ref: 25] Configuration loaded!");
            Ok(ctx)
        },
    }
}

fn main() -> Result<(), u32>
{
    use std::sync::Arc;

    const INIT_ERR_CODE: u32 = 1;
    const CODE_GEN_ERR_CODE: u32 = 2;

    SimpleLogger::new()
        .with_level(LevelFilter::Info)
        .init()
        .unwrap();

    let args = ProgArgs::parse();

    let app_context_parsed = setup_context(&args.config, args.check);

    let app_context = match app_context_parsed
    {
        Err(_e) => return Err(INIT_ERR_CODE),
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
        error!("[ref: 26] Failed to register signal handler");
        return Err(INIT_ERR_CODE);
    }

    if app_context.check_mode
    {
        info!("[ref: 27] Running in check mode");

        if let Err(err) = codegen::generate::check_references(&app_context)
        {
            error!("[ref: 28] Failed: {}", err);
            return Err(CODE_GEN_ERR_CODE);
        }
    }
    else
    {
        info!("[ref: 29] Running in code generation mode");

        if let Err(err) = codegen::generate::generate_code(&app_context)
        {
            error!("[ref: 30] Failed: {}", err);
            return Err(CODE_GEN_ERR_CODE);
        }
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
