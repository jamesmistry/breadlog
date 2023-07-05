use serde::{Deserialize, Serialize};
use serde_yaml;
use std::str::FromStr;
use std::sync::atomic;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RustLogMacro
{
    pub module: String,
    pub name: String,
}

#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct RustConfig
{
    pub log_macros: Vec<RustLogMacro>,

    #[serde(default = "default_rust_extensions")]
    pub extensions: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Config
{
    #[serde(skip)]
    pub config_dir: String,

    pub source_dir: String,

    #[serde(default)]
    pub rust: RustConfig,
}

pub struct Context
{
    pub config: Config,
    pub check_mode: bool,
    pub stop_commanded: Arc<atomic::AtomicBool>,
}

impl Context
{
    pub fn new(yaml: String, config_dir: &str, check_mode: bool) -> Result<Self, String>
    {
        match serde_yaml::from_str(&yaml)
        {
            Ok(loaded_config) =>
            {
                use std::path;

                let mut loaded_context = Self {
                    config: loaded_config,
                    check_mode,
                    stop_commanded: Arc::new(atomic::AtomicBool::new(false)),
                };

                match String::from_str(config_dir)
                {
                    Ok(dir) => loaded_context.config.config_dir = dir,
                    Err(e) => return Err(e.to_string()),
                }

                if !loaded_context
                    .config
                    .source_dir
                    .starts_with(std::path::MAIN_SEPARATOR)
                {
                    // It's a relative path so prepend the config dir
                    match path::Path::new(&loaded_context.config.config_dir)
                        .join(&loaded_context.config.source_dir)
                        .to_str()
                    {
                        None =>
                        {
                            return Err("Failed to make configuration path absolute".to_string())
                        },
                        Some(p) => loaded_context.config.source_dir = p.to_string(),
                    }
                }

                Ok(loaded_context)
            },

            Err(e) => Err(e.to_string()),
        }
    }
}

fn default_rust_extensions() -> Vec<String>
{
    vec!["rs".to_string()]
}

#[cfg(test)]
mod tests
{
    use super::Context;

    #[test]
    fn test_invalid_config()
    {
        let test_input = r#"
        rust:
          log_macros:
            - module: test_module
              name: test_macro
            - module: test_module
              name: test_macro1
            - module: test_module
              name: test_macro2
            - module: test_module::test_inner
              name: test_macro3
        "#;

        let config_dir = "/tmp".to_string();

        let subject = Context::new(test_input.to_string(), &config_dir, true);

        assert!(subject.is_err());
    }

    #[test]
    fn test_absolute_config_path()
    {
        let test_input = r#"
        source_dir: /tmp
        rust:
          log_macros:
            - module: test_module
              name: test_macro
            - module: test_module
              name: test_macro1
            - module: test_module
              name: test_macro2
            - module: test_module::test_inner
              name: test_macro3
        "#;

        let config_dir = "/tmp/test".to_string();

        let subject = Context::new(test_input.to_string(), &config_dir, true);

        assert!(subject.is_ok());
        assert_eq!(subject.unwrap().config.source_dir, "/tmp".to_string());
    }

    #[test]
    fn test_relative_config_path()
    {
        let test_input = r#"
        source_dir: test/dir
        rust:
          log_macros:
            - module: test_module
              name: test_macro
            - module: test_module
              name: test_macro1
            - module: test_module
              name: test_macro2
            - module: test_module::test_inner
              name: test_macro3
        "#;

        let config_dir = "/tmp".to_string();

        let subject = Context::new(test_input.to_string(), &config_dir, true);

        assert!(subject.is_ok());
        assert_eq!(
            subject.unwrap().config.source_dir,
            "/tmp/test/dir".to_string()
        );
    }
}
