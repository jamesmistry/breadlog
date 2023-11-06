use lazy_static::lazy_static;
use regex::Regex;
use std::vec::Vec;

use super::rust_parser::rust_log_ref_finder;
use crate::config::Config;

/// Represents a position in the source code.
#[derive(Copy, Clone)]
pub struct CodePosition
{
    /// The 0-based character offset from the start of the source code.
    character: usize,

    /// The 1-based line number in the source code.
    line: usize,

    /// The 1-based column number in the source code.
    column: usize,
}

/// Represents a log reference in the source code.
#[derive(Clone)]
pub struct LogRefEntry
{
    /// The position of the log reference in the source code.
    position: CodePosition,

    /// The numeric reference associated with the log message, if one exists.
    reference: Option<u32>,

    /// The name of the macro used to log the message.
    _macro_name: String,
}

/// Represents a programming language.
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Debug)]
pub enum CodeLanguage
{
    Rust = 0,
}

/// Finds log references in source code.
///
/// # Arguments
///
/// * `language` - The programming language of the source code.
/// * `code` - The source code to search for log references.
/// * `config` - The configuration to use when searching for log references.
///
/// # Returns
///
/// A vector of log references found in the source code.
pub fn find_references(language: CodeLanguage, code: &str, config: &Config) -> Vec<LogRefEntry>
{
    match language
    {
        CodeLanguage::Rust => rust_log_ref_finder::find(code, config),
    }
}

/// Returns the programming language of the source code.
impl CodePosition
{
    /// Creates a new CodePosition.
    ///
    /// # Arguments
    ///
    /// * `character` - The 0-based character offset from the start of the source code.
    /// * `line` - The 1-based line number in the source code.
    /// * `column` - The 1-based column number in the source code.
    pub fn new(character: usize, line: usize, column: usize) -> CodePosition
    {
        CodePosition {
            character,
            line,
            column,
        }
    }

    /// Returns the 0-based character offset from the start of the source code.
    pub fn character(&self) -> usize
    {
        self.character
    }

    /// Returns the 1-based line number in the source code.
    pub fn line(&self) -> usize
    {
        self.line
    }

    /// Returns the 1-based column number in the source code.
    pub fn column(&self) -> usize
    {
        self.column
    }
}

impl LogRefEntry
{
    /// Creates a new LogRefEntry.
    ///
    /// # Arguments
    ///
    /// * `position` - The position of the log reference in the source code.
    /// * `reference` - The numeric reference associated with the log message, if one exists.
    /// * `_macro_name` - The name of the macro used to log the message.
    pub fn new(position: CodePosition, reference: Option<u32>, _macro_name: String) -> LogRefEntry
    {
        LogRefEntry {
            position,
            reference,
            _macro_name,
        }
    }

    /// Returns the numeric reference associated with the log message, if one exists.
    ///
    /// Numeric references take the following form: `[ref: 1234]`
    ///
    /// # Arguments
    ///
    /// * `log_literal` - A string slice containing the first string literal passed to the logging function.
    pub fn extract_reference(log_literal: &str) -> Option<u32>
    {
        lazy_static! {
            static ref LOG_REF_PATTERN: Regex = Regex::new(r"^\[ref: ([0-9]{1,10})\]").unwrap();
        }

        if let Some(capture) = LOG_REF_PATTERN.captures_iter(log_literal).next()
        {
            let parsed_ref = capture[1].parse::<u32>();

            match parsed_ref
            {
                Err(_e) => return None,
                Ok(e) => return Some(e),
            };
        }

        None
    }

    /// Is a valid log reference present? Returns true if a valid log reference is present, false otherwise.
    pub fn exists(&self) -> bool
    {
        self.reference.is_some()
    }

    /// Returns the position of the log reference in the source code.
    pub fn position(&self) -> &CodePosition
    {
        &self.position
    }

    /// Returns the numeric reference associated with the log message, if one exists.
    pub fn reference(&self) -> Option<u32>
    {
        self.reference
    }

    /// Returns the name of the macro used to log the message.
    pub fn _macro_name(&self) -> &str
    {
        self._macro_name.as_str()
    }
}

mod tests
{
    #![allow(unused_imports)]
    use crate::parser::CodePosition;
    use crate::parser::LogRefEntry;

    #[test]
    fn test_logref_not_exists()
    {
        use std::str::FromStr;

        let subject = LogRefEntry::new(
            CodePosition {
                character: 10,
                line: 5,
                column: 2,
            },
            None,
            String::from_str("test_macro").unwrap(),
        );

        assert_eq!(subject.exists(), false);
        assert_eq!(subject.reference(), None);
        assert_eq!(subject.position().character(), 10);
        assert_eq!(subject.position().line(), 5);
        assert_eq!(subject.position().column(), 2);
        assert_eq!(subject._macro_name(), "test_macro");
    }

    #[test]
    fn test_logref_exists()
    {
        use std::str::FromStr;

        let subject = LogRefEntry::new(
            CodePosition {
                character: 10,
                line: 5,
                column: 2,
            },
            Some(1024),
            String::from_str("test_macro").unwrap(),
        );

        assert!(subject.exists());
        assert_eq!(subject.reference(), Some(1024));
        assert_eq!(subject.position().character(), 10);
        assert_eq!(subject.position().line(), 5);
        assert_eq!(subject.position().column(), 2);
        assert_eq!(subject._macro_name(), "test_macro");
    }

    #[test]
    fn test_logref_extract_exists()
    {
        let test_data = String::from("[ref: 1234] Test log message.");
        let test_slice = &test_data[0..test_data.len()];

        assert_eq!(LogRefEntry::extract_reference(test_slice), Some(1234));
    }

    #[test]
    fn test_logref_extract_does_not_exist()
    {
        let test_data = String::from("Test log message.");
        let test_slice = &test_data[0..test_data.len()];

        assert_eq!(LogRefEntry::extract_reference(test_slice), None);
    }

    #[test]
    fn test_logref_extract_ref_not_leading()
    {
        let test_data = String::from("Test log message. [ref: 1234]");
        let test_slice = &test_data[0..test_data.len()];

        assert_eq!(LogRefEntry::extract_reference(test_slice), None);
    }

    #[test]
    fn test_logref_extract_ref_not_numeric()
    {
        let test_data = String::from("[ref: 1bc2e] Test log message.");
        let test_slice = &test_data[0..test_data.len()];

        assert_eq!(LogRefEntry::extract_reference(test_slice), None);
    }

    #[test]
    fn test_logref_extract_ref_no_brackets()
    {
        let test_data = String::from("ref: 1234 Test log message.");
        let test_slice = &test_data[0..test_data.len()];

        assert_eq!(LogRefEntry::extract_reference(test_slice), None);
    }

    #[test]
    fn test_logref_extract_ref_min_val()
    {
        let test_data = String::from("[ref: 0] Test log message.");
        let test_slice = &test_data[0..test_data.len()];

        assert_eq!(LogRefEntry::extract_reference(test_slice), Some(0));
    }

    #[test]
    fn test_logref_extract_ref_max_val()
    {
        let test_data = String::from("[ref: 4294967295] Test log message.");
        let test_slice = &test_data[0..test_data.len()];

        assert_eq!(LogRefEntry::extract_reference(test_slice), Some(4294967295));
    }

    #[test]
    fn test_logref_extract_ref_multi_ref()
    {
        let test_data = String::from("[ref: 1234] [ref: 5678] Test log message.");
        let test_slice = &test_data[0..test_data.len()];

        assert_eq!(LogRefEntry::extract_reference(test_slice), Some(1234));
    }
}
