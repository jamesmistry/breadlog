use serde::{Deserialize, Serialize};
use serde_yaml;
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
    pub fn new(yaml: String, check_mode: bool) -> Result<Self, String>
    {
        match serde_yaml::from_str(&yaml)
        {
            Ok(loaded_config) => Ok(Self {
                config: loaded_config,
                check_mode: check_mode,
                stop_commanded: Arc::new(atomic::AtomicBool::new(false)),
            }),

            Err(e) => Err(e.to_string()),
        }
    }
}

fn default_rust_extensions() -> Vec<String>
{
    vec!["rs".to_string()]
}
