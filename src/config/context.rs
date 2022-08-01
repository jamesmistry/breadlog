use serde::{Deserialize, Serialize};
use serde_yaml;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Config
{
    pub source_dir: String,
}

pub struct Context
{
    pub config: Config,
}

impl Context
{
    pub fn new(yaml: String) -> Result<Self, String>
    {
        match serde_yaml::from_str(&yaml)
        {
            Ok(loaded_config) => Ok(Self {
                config: loaded_config,
            }),

            Err(e) => Err(e.to_string()),
        }
    }
}
