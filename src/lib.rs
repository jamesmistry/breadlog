/*
 * This library is used only to expose the code parser for fuzz testing.
 */

extern crate pest;
#[macro_use]
extern crate pest_derive;

use crate::parser::code_parser::CodeLanguage;
use crate::parser::LogRefEntry;

mod config;
mod parser;

fn test_config(yaml: &str) -> config::Config
{
    let config_dir = "/tmp".to_string();

    let ctx = config::Context::new(yaml.to_string(), &config_dir, true).unwrap();

    ctx.config
}

pub fn parse_rust(code: &str) -> Vec<LogRefEntry>
{
    let config = test_config(
        r#"
    source_dir: /tmp
    rust:
      log_macros:
      - module: log
        name: info
      - module: log
        name: warn
      - module: log
        name: error
"#,
    );

    parser::code_parser::find_references(CodeLanguage::Rust, code, &config)
}
