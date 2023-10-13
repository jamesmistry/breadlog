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

    #[serde(default = "default_use_cache")]
    pub use_cache: bool,

    #[serde(default)]
    pub rust: RustConfig,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cache
{
    pub next_reference_id: u32,
}

pub struct Context
{
    pub config: Config,
    pub cached_next_reference_id: Option<u32>,
    pub check_mode: bool,
    pub stop_commanded: Arc<atomic::AtomicBool>,
}

impl Context
{
    const CACHE_FILENAME: &str = "Breadlog.lock";
    const CACHE_EDIT_WARNING: &str = "# AUTO-GENERATED FILE - DON'T EDIT\n# If you would like to recalculate the next reference from your code, delete this file and\n# run Breadlog.\n\n";

    pub fn new(yaml: String, config_dir: &str, check_mode: bool) -> Result<Self, String>
    {
        match serde_yaml::from_str(&yaml)
        {
            Ok(loaded_config) =>
            {
                use std::path;

                let next_reference_id =
                    Context::read_cached_next_reference_id(&loaded_config, config_dir);

                let mut loaded_context = Self {
                    config: loaded_config,
                    cached_next_reference_id: next_reference_id,
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

    fn read_cached_next_reference_id(config: &Config, directory_path: &str) -> Option<u32>
    {
        let cache_path = std::path::Path::new(directory_path).join(Context::CACHE_FILENAME);

        if !config.use_cache || !cache_path.exists()
        {
            return None;
        }

        if let Ok(cache_yaml) = std::fs::read_to_string(cache_path)
        {
            match serde_yaml::from_str::<Cache>(cache_yaml.as_str())
            {
                Ok(loaded_cache) => Some(loaded_cache.next_reference_id),
                Err(e) =>
                {
                    log::warn!(
                        "Failed to parse lock file {}: {}",
                        Context::CACHE_FILENAME,
                        e
                    );
                    None
                },
            }
        }
        else
        {
            log::warn!("Failed to read lock file {}", Context::CACHE_FILENAME);
            None
        }
    }

    pub fn cache_next_reference_id(&self, id: u32, directory_path: &str)
    {
        if !self.config.use_cache
        {
            return;
        }

        let cache_path = std::path::Path::new(directory_path).join(Context::CACHE_FILENAME);

        let cache = Cache {
            next_reference_id: id,
        };

        match serde_yaml::to_string(&cache)
        {
            Ok(mut yaml) =>
            {
                yaml.insert_str(0, Context::CACHE_EDIT_WARNING);

                if let Err(e) = std::fs::write(cache_path, yaml)
                {
                    log::warn!(
                        "Failed to write lock file {}: {}",
                        Context::CACHE_FILENAME,
                        e
                    );
                }
            },
            Err(e) => log::warn!(
                "Failed to serialize lock file {}: {}",
                Context::CACHE_FILENAME,
                e
            ),
        }
    }
}

fn default_rust_extensions() -> Vec<String>
{
    vec!["rs".to_string()]
}

fn default_use_cache() -> bool
{
    true
}

#[cfg(test)]
mod tests
{
    use super::Context;

    use tempdir::TempDir;

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

    #[test]
    fn test_cache_load_no_cache()
    {
        let test_input = r#"
        source_dir: test/dir
        rust:
          log_macros:
            - module: test_module
              name: test_macro
        "#;

        let temp_dir = TempDir::new("breadlog_test").unwrap();
        let temp_dir_str = temp_dir.path().to_str().unwrap();

        let subject = Context::new(test_input.to_string(), temp_dir_str, true).unwrap();

        assert!(subject.cached_next_reference_id.is_none());
    }

    #[test]
    fn test_cache_load_valid_cache()
    {
        let test_input = r#"
        source_dir: test/dir
        rust:
          log_macros:
            - module: test_module
              name: test_macro
        "#;

        let temp_dir = TempDir::new("breadlog_test").unwrap();
        let cache_file = temp_dir.path().join(Context::CACHE_FILENAME);
        std::fs::write(cache_file, "---\nnext_reference_id: 123\n").unwrap();

        let temp_dir_str = temp_dir.path().to_str().unwrap();

        let subject = Context::new(test_input.to_string(), temp_dir_str, true).unwrap();

        assert_eq!(subject.cached_next_reference_id, Some(123));
    }

    #[test]
    fn test_cache_load_invalid_cache()
    {
        let test_input = r#"
        source_dir: test/dir
        rust:
          log_macros:
            - module: test_module
              name: test_macro
        "#;

        let temp_dir = TempDir::new("breadlog_test").unwrap();
        let cache_file = temp_dir.path().join(Context::CACHE_FILENAME);
        std::fs::write(cache_file, "/\\/ Invalid YAML /\\/").unwrap();

        let temp_dir_str = temp_dir.path().to_str().unwrap();

        let subject = Context::new(test_input.to_string(), temp_dir_str, true).unwrap();

        assert!(subject.cached_next_reference_id.is_none());
    }

    #[test]
    fn test_cache_disabled_read()
    {
        let test_input = r#"
        source_dir: test/dir
        use_cache: false
        rust:
          log_macros:
            - module: test_module
              name: test_macro
        "#;

        let temp_dir = TempDir::new("breadlog_test").unwrap();
        let cache_file = temp_dir.path().join(Context::CACHE_FILENAME);
        std::fs::write(cache_file, "---\nnext_reference_id: 123\n").unwrap();

        let temp_dir_str = temp_dir.path().to_str().unwrap();

        let subject = Context::new(test_input.to_string(), temp_dir_str, true).unwrap();

        assert!(subject.cached_next_reference_id.is_none());
    }

    #[test]
    fn test_cache_write()
    {
        let test_input = r#"
        source_dir: test/dir
        rust:
          log_macros:
            - module: test_module
              name: test_macro
        "#;

        let temp_dir = TempDir::new("breadlog_test").unwrap();
        let temp_dir_str = temp_dir.path().to_str().unwrap();
        let cache_file = temp_dir.path().join(Context::CACHE_FILENAME);

        {
            let ctx = Context::new(test_input.to_string(), temp_dir_str, true).unwrap();
            ctx.cache_next_reference_id(123, temp_dir.path().to_str().unwrap());
        }

        let cache_file_contents = std::fs::read_to_string(cache_file).unwrap();
        assert!(cache_file_contents.starts_with(Context::CACHE_EDIT_WARNING));

        let subject = Context::new(test_input.to_string(), temp_dir_str, true).unwrap();
        assert_eq!(subject.cached_next_reference_id, Some(123));
    }

    #[test]
    fn test_cache_disabled_write()
    {
        let test_input = r#"
        source_dir: test/dir
        use_cache: false
        rust:
          log_macros:
            - module: test_module
              name: test_macro
        "#;

        let temp_dir = TempDir::new("breadlog_test").unwrap();
        let temp_dir_str = temp_dir.path().to_str().unwrap();
        let cache_file = temp_dir.path().join(Context::CACHE_FILENAME);

        {
            let ctx = Context::new(test_input.to_string(), temp_dir_str, true).unwrap();
            ctx.cache_next_reference_id(123, temp_dir.path().to_str().unwrap());
        }

        assert_eq!(cache_file.exists(), false);

        let subject = Context::new(test_input.to_string(), temp_dir_str, true).unwrap();
        assert_eq!(subject.cached_next_reference_id, None);
    }
}
