use lazy_static::lazy_static;
use regex::Regex;

use super::rust_parser::rust_log_ref_finder;
use crate::config::Config;

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum LogRefKind
{
    Unknown,
    String,
    StructuredPreExisting,
    StructuredNew,
}

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

    /// The kind of log reference.
    kind: LogRefKind,

    /// Characters to insert at the specified position, before the reference ID.
    insertion_prefix: Option<String>,

    /// Characters to insert after the reference ID.
    insertion_suffix: Option<String>,
}

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

/// Determines if a log message present at the specified position is preceded
/// by a boolean directive flag in a comment.
///
/// # Arguments
///
/// * `directive_name` - The name of the directive to search for.
/// * `code` - The source code containing the log message.
/// * `subject_pos` - The character index in the source code of the first character of the log statement.
/// * `line_comment_extractor` - A regular expression that matches single-line comments in the source code.
///
/// # Returns
///
/// True if the flag is present, false otherwise.
fn check_for_boolean_directive(
    directive_name: &str,
    code: &str,
    subject_pos: usize,
    line_comment_extractor: &Regex,
) -> bool
{
    /*
     * Backtrack from subject_pos (character index in code) until a previous non-empty line is found.
     *
     * If the non-empty line contains a comment (as defined by the caller's regular expression), and that
     * comment's text specifies the ignore directive, return true. Otherwise, return false.
     */

    let mut first_line = true;

    for line in code[..subject_pos + 1].lines().rev()
    {
        if first_line
        {
            first_line = false;
            continue;
        }

        let line = line.trim();

        if line.is_empty()
        {
            continue;
        }

        match line_comment_extractor.captures(line)
        {
            None => break,
            Some(capture) =>
            {
                for group in capture.iter()
                {
                    match group
                    {
                        None => continue,
                        Some(comment) =>
                        {
                            if comment.as_str().to_lowercase().trim() == directive_name
                            {
                                return true;
                            }
                        },
                    }
                }

                break;
            },
        }
    }

    false
}

/// Determines if the specified code position is influenced by an "ignore"
/// directive. An ignore directive causes the affected line to be ignored for
/// the purposes of extracting and inserting log references.
///
/// # Arguments
///
/// * `code` - The source code containing the log message.
/// * `subject_pos` - The character index in the source code of the first character of the log statement.
/// * `line_comment_extractor` - A regular expression that matches single-line comments in the source code.
///
/// # Returns
///
/// True if the log message should be ignored, false otherwise.
pub fn check_for_ignore_directive(
    code: &str,
    subject_pos: usize,
    line_comment_extractor: &Regex,
) -> bool
{
    const IGNORE_DIRECTIVE_TEXT: &str = "breadlog:ignore";

    check_for_boolean_directive(
        IGNORE_DIRECTIVE_TEXT,
        code,
        subject_pos,
        line_comment_extractor,
    )
}

/// Determines if the specified code position is influenced by a "no KVP"
/// directive. A "no KVP" directive causes references on the affected line to
/// be extracted from and inserted into log message strings only, even when
/// in structured mode.
///
/// # Arguments
///
/// * `code` - The source code containing the log message.
/// * `subject_pos` - The character index in the source code of the first character of the log statement.
/// * `line_comment_extractor` - A regular expression that matches single-line comments in the source code.
///
/// # Returns
///
/// True if references should only be extracted from/inserted into log message strings, false otherwise.
pub fn check_for_no_kvp_directive(
    code: &str,
    subject_pos: usize,
    line_comment_extractor: &Regex,
) -> bool
{
    const NO_KVP_DIRECTIVE_TEXT: &str = "breadlog:no-kvp";

    check_for_boolean_directive(
        NO_KVP_DIRECTIVE_TEXT,
        code,
        subject_pos,
        line_comment_extractor,
    )
}

/// Returns the key used to identify a log reference in a key-value pair.
pub fn get_name_for_ref_kvp_key() -> &'static str
{
    lazy_static! {
        static ref REF_KVP_KEY: String = String::from("ref");
    }

    REF_KVP_KEY.as_str()
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
    pub fn new(
        position: CodePosition,
        reference: Option<u32>,
        _macro_name: String,
        kind: LogRefKind,
        insertion_prefix: Option<String>,
        insertion_suffix: Option<String>,
    ) -> LogRefEntry
    {
        LogRefEntry {
            position,
            reference,
            _macro_name,
            kind,
            insertion_prefix,
            insertion_suffix,
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

    /// Returns the kind of log reference.
    #[allow(dead_code)]
    pub fn kind(&self) -> LogRefKind
    {
        self.kind
    }

    /// It's possible that for structured log messages, the key to hold the
    /// reference is present but the value associated with it isn't usable,
    /// for example because it's not an integer type. In this case, this
    /// function returns false.
    pub fn usable_reference_position(&self) -> bool
    {
        if self.kind == LogRefKind::StructuredPreExisting && !self.exists()
        {
            return false;
        }

        true
    }

    /// Returns a string representation of the log reference suitable for
    /// inclusion in code. The language-specific parser determines the format
    /// unless the reference is being inserted into a string literal, in
    /// which case the canonical "[ref: 1234]" format is used.
    ///
    /// # Arguments
    ///
    /// * `reference_id` - The numerical ID to insert.
    pub fn insertable_reference_string(&self, reference_id: u32) -> String
    {
        let mut result = String::new();

        if self.insertion_prefix.is_none() && self.insertion_suffix.is_none()
        {
            /*
             * By default, produce a language-agnostic reference string for
             * inclusion in a log message string literal.
             */
            result.push_str(&format!("[ref: {}] ", reference_id));
        }
        else
        {
            if let Some(prefix) = &self.insertion_prefix
            {
                result.push_str(prefix);
            }

            result.push_str(format!("{}", reference_id).as_str());

            if let Some(suffix) = &self.insertion_suffix
            {
                result.push_str(suffix);
            }
        }

        result
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
    use crate::parser::check_for_ignore_directive;
    use crate::parser::check_for_no_kvp_directive;
    use crate::parser::code_parser::LogRefKind;
    use crate::parser::CodePosition;
    use crate::parser::LogRefEntry;
    use regex::Regex;

    #[allow(dead_code)]
    fn get_comment_extractor() -> Regex
    {
        Regex::new(r"\/\/(.+)").unwrap()
    }

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
            LogRefKind::String,
            None,
            None,
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
            LogRefKind::String,
            None,
            None,
        );

        assert!(subject.exists());
        assert_eq!(subject.reference(), Some(1024));
        assert_eq!(subject.position().character(), 10);
        assert_eq!(subject.position().line(), 5);
        assert_eq!(subject.position().column(), 2);
        assert_eq!(subject._macro_name(), "test_macro");
    }

    #[test]
    fn test_logref_usable_present_string()
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
            LogRefKind::String,
            None,
            None,
        );

        assert!(subject.usable_reference_position());
    }

    #[test]
    fn test_logref_usable_not_present_string()
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
            LogRefKind::String,
            None,
            None,
        );

        assert!(subject.usable_reference_position());
    }

    #[test]
    fn test_logref_usable_present_structured_new()
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
            LogRefKind::StructuredNew,
            None,
            None,
        );

        assert!(subject.usable_reference_position());
    }

    #[test]
    fn test_logref_usable_not_present_structured_new()
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
            LogRefKind::StructuredNew,
            None,
            None,
        );

        assert!(subject.usable_reference_position());
    }

    #[test]
    fn test_logref_usable_present_structured_existing()
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
            LogRefKind::StructuredPreExisting,
            None,
            None,
        );

        assert!(subject.usable_reference_position());
    }

    #[test]
    fn test_logref_usable_not_present_structured_existing()
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
            LogRefKind::StructuredPreExisting,
            None,
            None,
        );

        assert_eq!(subject.usable_reference_position(), false);
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

    #[test]
    fn test_ignore_directive_no_comment()
    {
        let comment_pattern = get_comment_extractor();

        let test_data =
            String::from("not_a_comment();\ntest_macro!(\"[ref: 1234] Test string.\");\n");
        let test_slice = &test_data[0..test_data.len()];

        assert_eq!(
            check_for_ignore_directive(test_slice, 17, &comment_pattern),
            false
        );
    }

    #[test]
    fn test_ignore_directive_empty_comment()
    {
        let comment_pattern = get_comment_extractor();

        let test_data = String::from("//\ntest_macro!(\"[ref: 1234] Test string.\");\n");
        let test_slice = &test_data[0..test_data.len()];

        assert_eq!(
            check_for_ignore_directive(test_slice, 3, &comment_pattern),
            false
        );
    }

    #[test]
    fn test_ignore_directive_non_directive_comment()
    {
        let comment_pattern = get_comment_extractor();

        let test_data =
            String::from("// Irrelevant comment.\ntest_macro!(\"[ref: 1234] Test string.\");\n");
        let test_slice = &test_data[0..test_data.len()];

        assert_eq!(
            check_for_ignore_directive(test_slice, 23, &comment_pattern),
            false
        );
    }

    #[test]
    fn test_ignore_directive_non_preceding_comment()
    {
        let comment_pattern = get_comment_extractor();

        let test_data = String::from("// breadlog:ignore\n// Irrelevant comment.\ntest_macro!(\"[ref: 1234] Test string.\");\n");
        let test_slice = &test_data[0..test_data.len()];

        assert_eq!(
            check_for_ignore_directive(test_slice, 42, &comment_pattern),
            false
        );
    }

    #[test]
    fn test_ignore_directive_separated_by_non_comment()
    {
        let comment_pattern = get_comment_extractor();

        let test_data = String::from(
            "// breadlog:ignore\nirrelevant_code();\ntest_macro!(\"[ref: 1234] Test string.\");\n",
        );
        let test_slice = &test_data[0..test_data.len()];

        assert_eq!(
            check_for_ignore_directive(test_slice, 38, &comment_pattern),
            false
        );
    }

    #[test]
    fn test_ignore_directive_present()
    {
        let comment_pattern = get_comment_extractor();

        let test_data =
            String::from("// breadlog:ignore\ntest_macro!(\"[ref: 1234] Test string.\");\n");
        let test_slice = &test_data[0..test_data.len()];

        assert!(check_for_ignore_directive(test_slice, 19, &comment_pattern));
    }

    #[test]
    fn test_ignore_directive_present_ignore_comment_whitespace()
    {
        let comment_pattern = get_comment_extractor();

        let test_data =
            String::from("//    breadlog:ignore    \ntest_macro!(\"[ref: 1234] Test string.\");\n");
        let test_slice = &test_data[0..test_data.len()];

        assert!(check_for_ignore_directive(test_slice, 26, &comment_pattern));
    }

    #[test]
    fn test_ignore_directive_present_case_neutral()
    {
        let comment_pattern = get_comment_extractor();

        let test_data =
            String::from("// BReadLOG:IGnoRE\ntest_macro!(\"[ref: 1234] Test string.\");\n");
        let test_slice = &test_data[0..test_data.len()];

        assert!(check_for_ignore_directive(test_slice, 19, &comment_pattern));
    }

    #[test]
    fn test_ignore_directive_present_ignore_line_whitespace()
    {
        let comment_pattern = get_comment_extractor();

        let test_data = String::from(
            "// breadlog:ignore\n   \n\n \n\ntest_macro!(\"[ref: 1234] Test string.\");\n",
        );
        let test_slice = &test_data[0..test_data.len()];

        assert!(check_for_ignore_directive(test_slice, 27, &comment_pattern));
    }

    #[test]
    fn test_ignore_directive_multi_capture()
    {
        let comment_pattern = Regex::new(r"\/\/(.+)|\/\*(.+)\*\/").unwrap();

        {
            let test_data =
                String::from("// breadlog:ignore\ntest_macro!(\"[ref: 1234] Test string.\");\n");
            let test_slice = &test_data[0..test_data.len()];

            assert!(check_for_ignore_directive(test_slice, 19, &comment_pattern));
        }

        {
            let test_data =
                String::from("/* breadlog:ignore */\ntest_macro!(\"[ref: 1234] Test string.\");\n");
            let test_slice = &test_data[0..test_data.len()];

            assert!(check_for_ignore_directive(test_slice, 22, &comment_pattern));
        }
    }

    #[test]
    fn test_no_kvp_directive_present()
    {
        let comment_pattern = get_comment_extractor();

        let test_data =
            String::from("// breadlog:no-kvp\ntest_macro!(\"[ref: 1234] Test string.\");\n");
        let test_slice = &test_data[0..test_data.len()];

        assert!(check_for_no_kvp_directive(test_slice, 19, &comment_pattern));
    }

    #[test]
    fn test_no_kvp_directive_not_present()
    {
        let comment_pattern = get_comment_extractor();

        let test_data =
            String::from("// breadlog:other\ntest_macro!(\"[ref: 1234] Test string.\");\n");
        let test_slice = &test_data[0..test_data.len()];

        assert_eq!(
            check_for_no_kvp_directive(test_slice, 19, &comment_pattern),
            false
        );
    }

    #[test]
    fn test_insertable_reference_default()
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
            LogRefKind::String,
            None,
            None,
        );

        assert_eq!(subject.insertable_reference_string(123), "[ref: 123] ");
    }

    #[test]
    fn test_insertable_reference_prefix_only()
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
            LogRefKind::String,
            Some("test-prefix: ".to_string()),
            None,
        );

        assert_eq!(subject.insertable_reference_string(123), "test-prefix: 123");
    }

    #[test]
    fn test_insertable_reference_suffix_only()
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
            LogRefKind::String,
            None,
            Some(" :test-suffix".to_string()),
        );

        assert_eq!(subject.insertable_reference_string(123), "123 :test-suffix");
    }

    #[test]
    fn test_insertable_reference_prefix_and_suffix()
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
            LogRefKind::String,
            Some("test-prefix: ".to_string()),
            Some(" :test-suffix".to_string()),
        );

        assert_eq!(
            subject.insertable_reference_string(123),
            "test-prefix: 123 :test-suffix"
        );
    }
}
