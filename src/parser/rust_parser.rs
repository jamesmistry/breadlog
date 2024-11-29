use super::check_for_ignore_directive;
use super::check_for_no_kvp_directive;
use super::get_name_for_ref_kvp_key;
use super::CodePosition;
use super::LogRefEntry;
use super::LogRefKind;
use crate::config::Config;
use lazy_static::lazy_static;
use pest::Parser;
use regex::Regex;
use std::str::FromStr;

#[derive(Parser)]
#[grammar = "parser/rust_grammar.pest"]
struct RustParser;

/// Finds all log references in the given code.
pub mod rust_log_ref_finder
{
    use super::*;

    fn macro_of_interest(macro_name: &String, config: &Config) -> bool
    {
        for config_macro in &config.rust.log_macros
        {
            if macro_name == config_macro.name.as_str()
            {
                return true;
            }
            else
            {
                let qualified_macro_name =
                    format!("{}::{}", config_macro.module, config_macro.name);

                if macro_name == qualified_macro_name.as_str()
                {
                    return true;
                }
            }
        }

        false
    }

    /// Finds all log references in the given code.
    ///
    /// # Arguments
    ///
    /// * `code` - The source code to search for log references.
    /// * `config` - The configuration to use when searching for log references.
    ///
    /// # Returns
    ///
    /// A vector of log references found in the source code.
    pub fn find(code: &str, config: &Config) -> Vec<LogRefEntry>
    {
        lazy_static! {
            static ref RUST_COMMENT_PATTERN: Regex = Regex::new(r"\/\/(.+)|\/\*(.+)\*\/").unwrap();
        }

        let mut result = Vec::new();

        let mut outer_most_parsed_target = match RustParser::parse(Rule::file, code)
        {
            Err(_) => return result,
            Ok(parsed) => parsed,
        };

        let parsed_target = match outer_most_parsed_target.next()
        {
            None => return result,
            Some(parsed) => parsed,
        };

        for found in parsed_target.into_inner()
        {
            match found.as_rule()
            {
                Rule::log_macro =>
                {
                    let mut inner_rules = found.into_inner();

                    // Macro name
                    let inner_rule = inner_rules.next();

                    let macro_name: &str = match inner_rule
                    {
                        None => continue,
                        Some(rule) =>
                        {
                            if rule.as_rule() != Rule::macro_name
                            {
                                continue;
                            }

                            if check_for_ignore_directive(
                                code,
                                rule.as_span().start(),
                                &RUST_COMMENT_PATTERN,
                            )
                            {
                                continue;
                            }

                            rule.as_str()
                        },
                    };

                    let macro_name_parsed = String::from_str(macro_name);
                    let macro_name_str = match macro_name_parsed
                    {
                        Err(_e) => continue,
                        Ok(name) => name,
                    };

                    if !macro_of_interest(&macro_name_str, config)
                    {
                        /*
                         * This isn't a macro specified in config.
                         */
                        continue;
                    }

                    // Macro arguments
                    let rule_l1 = inner_rules.next();

                    let rule_l2 = match rule_l1
                    {
                        None => continue,
                        Some(rule) =>
                        {
                            if rule.as_rule() != Rule::macro_args
                            {
                                continue;
                            }

                            rule
                        },
                    };

                    let mut ref_kind = LogRefKind::Unknown;

                    let mut log_message_span: Option<pest::Span> = None;
                    let rule_ref_container_span = rule_l2.as_span();
                    let mut kvp_spans: Vec<(pest::Span, Option<pest::Span>)> = Vec::new();

                    for rule in rule_l2.into_inner()
                    {
                        match rule.as_rule()
                        {
                            Rule::string_literal =>
                            {
                                log_message_span = match rule.into_inner().next()
                                {
                                    None => continue,
                                    Some(span) => Some(span.as_span()),
                                };
                            },
                            Rule::kvp_args =>
                            {
                                let kvps = rule.into_inner();

                                for kvp in kvps
                                {
                                    match kvp.as_rule()
                                    {
                                        Rule::kvp_key =>
                                        {
                                            kvp_spans.push((kvp.as_span(), None));
                                        },

                                        Rule::kvp_value => match kvp_spans.last_mut()
                                        {
                                            None => continue,
                                            Some((_, value_span)) =>
                                            {
                                                *value_span = Some(kvp.as_span());
                                            },
                                        },
                                        _ => continue,
                                    }
                                }
                            },
                            _ => continue,
                        }
                    }

                    let mut reference: Option<u32> = None;
                    let mut code_pos: Option<CodePosition> = None;
                    let mut insertion_prefix: Option<String> = None;
                    let mut insertion_suffix: Option<String> = None;

                    if config.rust.structured
                        && !check_for_no_kvp_directive(
                            code,
                            rule_ref_container_span.start(),
                            &RUST_COMMENT_PATTERN,
                        )
                    {
                        /*
                         * If treating this log message as structured, the
                         * reference needs to be represented as a key-value
                         * pair in the macro arguments.
                         */

                        let total_kvps = kvp_spans.len();
                        let ref_kvp_key: &str = get_name_for_ref_kvp_key();

                        /*
                         * Iterate over each key-value pair argument to find
                         * one that can hold a reference ID.
                         */
                        for (kvp_key, kvp_value) in kvp_spans
                        {
                            if kvp_key.as_str() == ref_kvp_key
                            {
                                match kvp_value
                                {
                                    None => continue,
                                    Some(span) =>
                                    {
                                        code_pos = Some(CodePosition::new(
                                            span.start(),
                                            span.start_pos().line_col().0,
                                            span.start_pos().line_col().1,
                                        ));

                                        ref_kind = LogRefKind::StructuredPreExisting;
                                        reference = match span.as_str().parse::<u32>()
                                        {
                                            Err(_) => None,
                                            Ok(val) => Some(val),
                                        };

                                        break;
                                    },
                                }
                            }
                        }

                        /*
                         * If no code position has yet been recorded, it means
                         * we've not yet found a KVP argument that can hold a
                         * reference ID. In this case a new one will have to be
                         * created.
                         */
                        if code_pos.is_none()
                        {
                            ref_kind = LogRefKind::StructuredNew;
                            insertion_prefix = Some(format!("{} = ", ref_kvp_key));

                            /*
                             * If there are other KVP arguments, the inserted
                             * one needs to terminate with a comma. Otherwise,
                             * if it's the only KVP argument, it needs to
                             * terminate with a semicolon.
                             */
                            if total_kvps > 0
                            {
                                insertion_suffix = Some(", ".to_string());
                            }
                            else
                            {
                                insertion_suffix = Some("; ".to_string());
                            }

                            code_pos = Some(CodePosition::new(
                                rule_ref_container_span.start() + 1,
                                rule_ref_container_span.start_pos().line_col().0,
                                rule_ref_container_span.start_pos().line_col().1 + 1,
                            ));
                        }
                    }
                    else
                    {
                        /*
                         * If treating this log message as unstructured, the
                         * reference needs to be represented as text in the
                         * message string.
                         */

                        match log_message_span
                        {
                            None => continue,
                            Some(span) =>
                            {
                                code_pos = Some(CodePosition::new(
                                    span.start(),
                                    span.start_pos().line_col().0,
                                    span.start_pos().line_col().1,
                                ));

                                ref_kind = LogRefKind::String;
                                reference =
                                    LogRefEntry::extract_reference(&code[span.start()..span.end()]);
                            },
                        }
                    }

                    let ref_entry = match code_pos
                    {
                        None => continue,
                        Some(pos) => LogRefEntry::new(
                            pos,
                            reference,
                            macro_name_str
                                .rfind("::")
                                .map_or(macro_name_str.clone(), |i| {
                                    macro_name_str[i + 2..].to_string()
                                }),
                            ref_kind,
                            insertion_prefix,
                            insertion_suffix,
                        ),
                    };

                    result.push(ref_entry);
                },
                Rule::EOI => (),
                _ => unreachable!(),
            }
        }

        result
    }
}

#[cfg(test)]
mod tests
{
    use super::rust_log_ref_finder;
    use super::LogRefEntry;
    use crate::config::Context;
    use std::str::FromStr;
    use test_log::test;

    fn create_test_context(structured_mode: bool) -> Context
    {
        let context = Context::new(
            r#"
source_dir: /tmp/test
rust:
  structured: {structured}
  log_macros:
    - module: test_module
      name: test_macro
    - module: test_module
      name: test_macro1
    - module: test_module
      name: test_macro2
    - module: test_module::test_inner
      name: test_macro3
"#
            .to_string()
            .replace("{structured}", &structured_mode.to_string()),
            &"/tmp".to_string(),
            false,
        )
        .unwrap();

        context
    }

    fn apply_grammar_to_string(test_data: &str, structured_mode: bool) -> Vec<LogRefEntry>
    {
        let test_data_string = String::from_str(test_data).unwrap();

        let ctx = create_test_context(structured_mode);

        rust_log_ref_finder::find(&test_data_string, &ctx.config)
    }

    #[test]
    fn test_grammar_single_line_literal()
    {
        let test_data = "test_macro!(\"Test string.\")\n";

        let found_macros = apply_grammar_to_string(test_data, false);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0]._macro_name(), "test_macro");
        assert_eq!(found_macros[0].position().character(), 13);
        assert_eq!(found_macros[0].position().line(), 1);
        assert_eq!(found_macros[0].position().column(), 14);
        assert_eq!(found_macros[0].reference(), None);
        assert_eq!(found_macros[0].kind(), super::LogRefKind::String);
    }

    #[test]
    fn test_grammar_invalid_reference()
    {
        let test_data = "test_macro!(\"[ref: abc] Test string.\")\n";

        let found_macros = apply_grammar_to_string(test_data, false);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0]._macro_name(), "test_macro");
        assert_eq!(found_macros[0].position().character(), 13);
        assert_eq!(found_macros[0].position().line(), 1);
        assert_eq!(found_macros[0].position().column(), 14);
        assert_eq!(found_macros[0].reference(), None);
        assert_eq!(found_macros[0].kind(), super::LogRefKind::String);
    }

    #[test]
    fn test_grammar_multiple_macros()
    {
        let test_data = "test_macro1!(\"Test string 1.\");\ntest_macro2!(\"Test string 2.\");\n";

        let found_macros = apply_grammar_to_string(test_data, false);

        assert_eq!(found_macros.len(), 2);
        assert_eq!(found_macros[0]._macro_name(), "test_macro1");
        assert_eq!(found_macros[0].position().character(), 14);
        assert_eq!(found_macros[0].position().line(), 1);
        assert_eq!(found_macros[0].position().column(), 15);
        assert_eq!(found_macros[0].reference(), None);
        assert_eq!(found_macros[0].kind(), super::LogRefKind::String);
        assert_eq!(found_macros[1]._macro_name(), "test_macro2");
        assert_eq!(found_macros[1].position().character(), 46);
        assert_eq!(found_macros[1].position().line(), 2);
        assert_eq!(found_macros[1].position().column(), 15);
        assert_eq!(found_macros[1].reference(), None);
        assert_eq!(found_macros[1].kind(), super::LogRefKind::String);
    }

    #[test]
    fn test_grammar_literal_escape_sequence()
    {
        let test_data =
            "test_macro1!(\"Test \\\"string 1.\");\ntest_macro2!(\"Test string 2.\");\n";

        let found_macros = apply_grammar_to_string(test_data, false);

        assert_eq!(found_macros.len(), 2);
        assert_eq!(found_macros[0]._macro_name(), "test_macro1");
        assert_eq!(found_macros[0].position().character(), 14);
        assert_eq!(found_macros[0].position().line(), 1);
        assert_eq!(found_macros[0].position().column(), 15);
        assert_eq!(found_macros[0].reference(), None);
        assert_eq!(found_macros[0].kind(), super::LogRefKind::String);
        assert_eq!(found_macros[1]._macro_name(), "test_macro2");
        assert_eq!(found_macros[1].position().character(), 48);
        assert_eq!(found_macros[1].position().line(), 2);
        assert_eq!(found_macros[1].position().column(), 15);
        assert_eq!(found_macros[1].reference(), None);
        assert_eq!(found_macros[1].kind(), super::LogRefKind::String);
    }

    #[test]
    fn test_grammar_ignore_strings()
    {
        let test_data = "\"test_macro1!(\\\"Test string\\\");\"\ntest_macro2!(\"Test string\");\n";

        let found_macros = apply_grammar_to_string(test_data, false);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0]._macro_name(), "test_macro2");
        assert_eq!(found_macros[0].position().character(), 47);
        assert_eq!(found_macros[0].position().line(), 2);
        assert_eq!(found_macros[0].position().column(), 15);
        assert_eq!(found_macros[0].reference(), None);
        assert_eq!(found_macros[0].kind(), super::LogRefKind::String);
    }

    #[test]
    fn test_grammar_recognise_escaped_quotes()
    {
        let test_data = "\"\\\"test_macro!(\\\"Test string\\\");\"\n";

        let found_macros = apply_grammar_to_string(test_data, false);

        assert!(found_macros.is_empty());
    }

    #[test]
    fn test_grammar_recognise_escaped_quotes_when_preceded_by_escape_char()
    {
        let test_data = "\"\\\\\\\"test_macro!(\\\"Test string\\\");\"\n";

        let found_macros = apply_grammar_to_string(test_data, false);

        assert!(found_macros.is_empty());
    }

    #[test]
    fn test_grammar_string_precedes_macro()
    {
        let test_data = "\"This is a string.\" test_macro!(\"Test string.\")\n";

        let found_macros = apply_grammar_to_string(test_data, false);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0]._macro_name(), "test_macro");
        assert_eq!(found_macros[0].position().character(), 33);
        assert_eq!(found_macros[0].position().line(), 1);
        assert_eq!(found_macros[0].position().column(), 34);
        assert_eq!(found_macros[0].reference(), None);
        assert_eq!(found_macros[0].kind(), super::LogRefKind::String);
    }

    #[test]
    fn test_grammar_string_not_closed_by_escaped_quote()
    {
        let test_data = "\"This is a string.\\\" test_macro!(\\\"Test arg.\\\")\"\n";

        let found_macros = apply_grammar_to_string(test_data, false);

        assert_eq!(found_macros.len(), 0);
    }

    #[test]
    fn test_grammar_macro_no_args()
    {
        let test_data = "test_macro!()\n";

        let found_macros = apply_grammar_to_string(test_data, false);

        assert_eq!(found_macros.len(), 0);
    }

    #[test]
    fn test_grammar_macro_no_string_args()
    {
        let test_data = "test_macro!(1234)\n";

        let found_macros = apply_grammar_to_string(test_data, false);

        assert_eq!(found_macros.len(), 0);
    }

    #[test]
    fn test_grammar_whitespace_before_literal()
    {
        let test_data = "test_macro!(    \"Test.\")\n";

        let found_macros = apply_grammar_to_string(test_data, false);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0]._macro_name(), "test_macro");
        assert_eq!(found_macros[0].position().character(), 17);
        assert_eq!(found_macros[0].position().line(), 1);
        assert_eq!(found_macros[0].position().column(), 18);
        assert_eq!(found_macros[0].reference(), None);
        assert_eq!(found_macros[0].kind(), super::LogRefKind::String);
    }

    #[test]
    fn test_grammar_comment_before_literal()
    {
        let test_data = "test_macro!(/* Test comment. */\"Test.\")\n";

        let found_macros = apply_grammar_to_string(test_data, false);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0]._macro_name(), "test_macro");
        assert_eq!(found_macros[0].position().character(), 32);
        assert_eq!(found_macros[0].position().line(), 1);
        assert_eq!(found_macros[0].position().column(), 33);
        assert_eq!(found_macros[0].reference(), None);
        assert_eq!(found_macros[0].kind(), super::LogRefKind::String);
    }

    #[test]
    fn test_grammar_multiline_comment_ignored()
    {
        let test_data = "/*\ntest_macro!(\"Test.\")\n*/\n";

        let found_macros = apply_grammar_to_string(test_data, false);

        assert_eq!(found_macros.len(), 0);
    }

    #[test]
    fn test_grammar_single_line_comment_ignored()
    {
        let test_data = "// test_macro!(\"Test.\")\n";

        let found_macros = apply_grammar_to_string(test_data, false);

        assert_eq!(found_macros.len(), 0);
    }

    #[test]
    fn test_grammar_extract_reference()
    {
        let test_data = "test_macro!(\"[ref: 1234] Test string.\")\n";

        let found_macros = apply_grammar_to_string(test_data, false);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0].reference(), Some(1234));
        assert_eq!(found_macros[0].kind(), super::LogRefKind::String);
    }

    #[test]
    fn test_unconfigured_macro()
    {
        let test_data = "random_macro!(\"[ref: 1234] Test string.\")\n";

        let found_macros = apply_grammar_to_string(test_data, false);

        assert_eq!(found_macros.len(), 0);
    }

    #[test]
    fn test_qualified_macro()
    {
        let test_data = "test_module::test_macro!(\"[ref: 1234] Test string.\")\n";

        let found_macros = apply_grammar_to_string(test_data, false);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0].reference(), Some(1234));
        assert_eq!(found_macros[0]._macro_name(), "test_macro");
        assert_eq!(found_macros[0].kind(), super::LogRefKind::String);
    }

    #[test]
    fn test_qualified_multipath_macro()
    {
        let test_data = "test_module::test_inner::test_macro3!(\"[ref: 1234] Test string.\")\n";

        let found_macros = apply_grammar_to_string(test_data, false);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0].reference(), Some(1234));
        assert_eq!(found_macros[0]._macro_name(), "test_macro3");
        assert_eq!(found_macros[0].kind(), super::LogRefKind::String);
    }

    #[test]
    fn test_grammar_target_arg()
    {
        let test_data = "test_macro!(target: \"test_target\", \"Test string.\")\n";

        let found_macros = apply_grammar_to_string(test_data, false);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0]._macro_name(), "test_macro");
        assert_eq!(found_macros[0].position().character(), 36);
        assert_eq!(found_macros[0].position().line(), 1);
        assert_eq!(found_macros[0].position().column(), 37);
        assert_eq!(found_macros[0].reference(), None);
        assert_eq!(found_macros[0].kind(), super::LogRefKind::String);
    }

    #[test]
    fn test_grammar_target_arg_multiple_macros()
    {
        let test_data = "test_macro1!(target: \"test_target1\", \"Test string 1.\");\ntest_macro2!(target: \"test_target2\", \"Test string 2.\");\n";

        let found_macros = apply_grammar_to_string(test_data, false);

        assert_eq!(found_macros.len(), 2);
        assert_eq!(found_macros[0]._macro_name(), "test_macro1");
        assert_eq!(found_macros[0].position().character(), 38);
        assert_eq!(found_macros[0].position().line(), 1);
        assert_eq!(found_macros[0].position().column(), 39);
        assert_eq!(found_macros[0].reference(), None);
        assert_eq!(found_macros[0].kind(), super::LogRefKind::String);
        assert_eq!(found_macros[1]._macro_name(), "test_macro2");
        assert_eq!(found_macros[1].position().character(), 94);
        assert_eq!(found_macros[1].position().line(), 2);
        assert_eq!(found_macros[1].position().column(), 39);
        assert_eq!(found_macros[1].reference(), None);
        assert_eq!(found_macros[1].kind(), super::LogRefKind::String);
    }

    #[test]
    fn test_grammar_target_arg_extract_reference()
    {
        let test_data = "test_macro!(target: \"test_target1\", \"[ref: 1234] Test string.\")\n";

        let found_macros = apply_grammar_to_string(test_data, false);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0].reference(), Some(1234));
        assert_eq!(found_macros[0].kind(), super::LogRefKind::String);
    }

    #[test]
    fn test_grammar_comment_handling()
    {
        let test_data = r#"
            /// Single-line comment.
            
            /*
                Multi-line comment.
             */

            /* Multi-line comment on a single line. */
            
            test_macro!("Reading configuration file: {}", config_filename);
"#;

        let found_macros = apply_grammar_to_string(test_data, false);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0].reference(), None);
        assert_eq!(found_macros[0].kind(), super::LogRefKind::String);
    }

    #[test]
    fn test_grammar_kvp_args_single_kvp_no_target()
    {
        let test_data = "test_macro!(a = 1; \"Test string.\")\n";

        let found_macros = apply_grammar_to_string(test_data, false);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0]._macro_name(), "test_macro");
        assert_eq!(found_macros[0].position().character(), 20);
        assert_eq!(found_macros[0].position().line(), 1);
        assert_eq!(found_macros[0].position().column(), 21);
        assert_eq!(found_macros[0].reference(), None);
        assert_eq!(found_macros[0].kind(), super::LogRefKind::String);
    }

    #[test]
    fn test_grammar_kvp_args_single_kvp_target()
    {
        let test_data = "test_macro!(target: \"test target\", a = 1; \"Test string.\")\n";

        let found_macros = apply_grammar_to_string(test_data, false);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0]._macro_name(), "test_macro");
        assert_eq!(found_macros[0].position().character(), 43);
        assert_eq!(found_macros[0].position().line(), 1);
        assert_eq!(found_macros[0].position().column(), 44);
        assert_eq!(found_macros[0].reference(), None);
        assert_eq!(found_macros[0].kind(), super::LogRefKind::String);
    }

    #[test]
    fn test_grammar_kvp_args_multi_kvp_no_target()
    {
        let test_data = "test_macro!(a = 1, b = 2; \"Test string.\")\n";

        let found_macros = apply_grammar_to_string(test_data, false);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0]._macro_name(), "test_macro");
        assert_eq!(found_macros[0].position().character(), 27);
        assert_eq!(found_macros[0].position().line(), 1);
        assert_eq!(found_macros[0].position().column(), 28);
        assert_eq!(found_macros[0].reference(), None);
        assert_eq!(found_macros[0].kind(), super::LogRefKind::String);
    }

    #[test]
    fn test_grammar_kvp_args_multi_kvp_target()
    {
        let test_data = "test_macro!(target: \"test target\", a = 1, b = 2; \"Test string.\")\n";

        let found_macros = apply_grammar_to_string(test_data, false);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0]._macro_name(), "test_macro");
        assert_eq!(found_macros[0].position().character(), 50);
        assert_eq!(found_macros[0].position().line(), 1);
        assert_eq!(found_macros[0].position().column(), 51);
        assert_eq!(found_macros[0].reference(), None);
        assert_eq!(found_macros[0].kind(), super::LogRefKind::String);
    }

    #[test]
    fn test_grammar_kvp_modifier_single_char()
    {
        let test_data = "test_macro!(a:? = 1; \"Test string.\")\n";

        let found_macros = apply_grammar_to_string(test_data, false);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0]._macro_name(), "test_macro");
        assert_eq!(found_macros[0].position().character(), 22);
        assert_eq!(found_macros[0].position().line(), 1);
        assert_eq!(found_macros[0].position().column(), 23);
        assert_eq!(found_macros[0].reference(), None);
        assert_eq!(found_macros[0].kind(), super::LogRefKind::String);
    }

    #[test]
    fn test_grammar_kvp_modifier_multi_char()
    {
        let test_data = "test_macro!(a:debug = 1; \"Test string.\")\n";

        let found_macros = apply_grammar_to_string(test_data, false);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0]._macro_name(), "test_macro");
        assert_eq!(found_macros[0].position().character(), 26);
        assert_eq!(found_macros[0].position().line(), 1);
        assert_eq!(found_macros[0].position().column(), 27);
        assert_eq!(found_macros[0].reference(), None);
        assert_eq!(found_macros[0].kind(), super::LogRefKind::String);
    }

    #[test]
    fn test_grammar_kvp_termination_with_literal_semicolon()
    {
        let test_data = "test_macro!(a = \"Test string 1;\"; \"Test string 2.\")\n";

        let found_macros = apply_grammar_to_string(test_data, false);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0]._macro_name(), "test_macro");
        assert_eq!(found_macros[0].position().character(), 35);
        assert_eq!(found_macros[0].position().line(), 1);
        assert_eq!(found_macros[0].position().column(), 36);
        assert_eq!(found_macros[0].reference(), None);
        assert_eq!(found_macros[0].kind(), super::LogRefKind::String);
    }

    #[test]
    fn test_grammar_kvp_termination_with_multi_kv_literal_semicolons()
    {
        let test_data =
            "test_macro!(a = \"Test string 1;\", b = \"Test string 2;\"; \"Test string 3.\")\n";

        let found_macros = apply_grammar_to_string(test_data, false);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0]._macro_name(), "test_macro");
        assert_eq!(found_macros[0].position().character(), 57);
        assert_eq!(found_macros[0].position().line(), 1);
        assert_eq!(found_macros[0].position().column(), 58);
        assert_eq!(found_macros[0].reference(), None);
        assert_eq!(found_macros[0].kind(), super::LogRefKind::String);
    }

    #[test]
    fn test_grammar_kvp_multi_string_in_value()
    {
        let test_data =
            "test_macro!(a = \"Test string 1\".cmp(\"Test string 2\"); \"Test string 3.\")\n";

        let found_macros = apply_grammar_to_string(test_data, false);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0]._macro_name(), "test_macro");
        assert_eq!(found_macros[0].position().character(), 55);
        assert_eq!(found_macros[0].position().line(), 1);
        assert_eq!(found_macros[0].position().column(), 56);
        assert_eq!(found_macros[0].reference(), None);
        assert_eq!(found_macros[0].kind(), super::LogRefKind::String);
    }

    #[test]
    fn test_grammar_kvp_multi_string_in_value_with_substatement_end_in_literal()
    {
        let test_data =
            "test_macro!(a = \"Test string 1\".cmp(\"Test string 2;\"); \"Test string 3.\")\n";

        let found_macros = apply_grammar_to_string(test_data, false);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0]._macro_name(), "test_macro");
        assert_eq!(found_macros[0].position().character(), 56);
        assert_eq!(found_macros[0].position().line(), 1);
        assert_eq!(found_macros[0].position().column(), 57);
        assert_eq!(found_macros[0].reference(), None);
        assert_eq!(found_macros[0].kind(), super::LogRefKind::String);
    }

    #[test]
    fn test_grammar_kvp_single_no_assignment()
    {
        let test_data = "test_macro!(a; \"Test string.\")\n";

        let found_macros = apply_grammar_to_string(test_data, false);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0]._macro_name(), "test_macro");
        assert_eq!(found_macros[0].position().character(), 16);
        assert_eq!(found_macros[0].position().line(), 1);
        assert_eq!(found_macros[0].position().column(), 17);
        assert_eq!(found_macros[0].reference(), None);
        assert_eq!(found_macros[0].kind(), super::LogRefKind::String);
    }

    #[test]
    fn test_grammar_kvp_multi_no_assignment()
    {
        let test_data = "test_macro!(a, b; \"Test string.\")\n";

        let found_macros = apply_grammar_to_string(test_data, false);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0]._macro_name(), "test_macro");
        assert_eq!(found_macros[0].position().character(), 19);
        assert_eq!(found_macros[0].position().line(), 1);
        assert_eq!(found_macros[0].position().column(), 20);
        assert_eq!(found_macros[0].reference(), None);
        assert_eq!(found_macros[0].kind(), super::LogRefKind::String);
    }

    #[test]
    fn test_grammar_kvp_multi_assignment_mix()
    {
        let test_data = "test_macro!(a = 1, b; \"Test string.\")\n";

        let found_macros = apply_grammar_to_string(test_data, false);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0]._macro_name(), "test_macro");
        assert_eq!(found_macros[0].position().character(), 23);
        assert_eq!(found_macros[0].position().line(), 1);
        assert_eq!(found_macros[0].position().column(), 24);
        assert_eq!(found_macros[0].reference(), None);
        assert_eq!(found_macros[0].kind(), super::LogRefKind::String);
    }

    #[test]
    fn test_line_comment_ignore_directive()
    {
        let test_data = "// breadlog:ignore\ntest_macro1!(\"Test string 1.\");\ntest_macro2!(\"Test string 2.\");\n";

        let found_macros = apply_grammar_to_string(test_data, false);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0]._macro_name(), "test_macro2");
        assert_eq!(found_macros[0].position().character(), 65);
        assert_eq!(found_macros[0].position().line(), 3);
        assert_eq!(found_macros[0].position().column(), 15);
        assert_eq!(found_macros[0].reference(), None);
        assert_eq!(found_macros[0].kind(), super::LogRefKind::String);
    }

    #[test]
    fn test_block_comment_ignore_directive()
    {
        let test_data = "/* breadlog:ignore */\ntest_macro1!(\"Test string 1.\");\ntest_macro2!(\"Test string 2.\");\n";

        let found_macros = apply_grammar_to_string(test_data, false);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0]._macro_name(), "test_macro2");
        assert_eq!(found_macros[0].position().character(), 68);
        assert_eq!(found_macros[0].position().line(), 3);
        assert_eq!(found_macros[0].position().column(), 15);
        assert_eq!(found_macros[0].reference(), None);
        assert_eq!(found_macros[0].kind(), super::LogRefKind::String);
    }

    #[test]
    fn test_grammar_kvp_structured_found_reference()
    {
        let test_data = "test_macro!(ref = 123; \"Test string.\")\n";

        let found_macros = apply_grammar_to_string(test_data, true);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0]._macro_name(), "test_macro");
        assert_eq!(found_macros[0].position().character(), 18);
        assert_eq!(found_macros[0].position().line(), 1);
        assert_eq!(found_macros[0].position().column(), 19);
        assert_eq!(found_macros[0].reference(), Some(123));
        assert_eq!(
            found_macros[0].kind(),
            super::LogRefKind::StructuredPreExisting
        );
    }

    #[test]
    fn test_grammar_kvp_structured_invalid_reference()
    {
        let test_data = "test_macro!(ref = \"abc\"; \"Test string.\")\n";

        let found_macros = apply_grammar_to_string(test_data, true);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0]._macro_name(), "test_macro");
        assert_eq!(found_macros[0].position().character(), 18);
        assert_eq!(found_macros[0].position().line(), 1);
        assert_eq!(found_macros[0].position().column(), 19);
        assert_eq!(found_macros[0].reference(), None);
        assert_eq!(
            found_macros[0].kind(),
            super::LogRefKind::StructuredPreExisting
        );
    }

    #[test]
    fn test_grammar_kvp_structured_no_reference_no_other_kvps()
    {
        let test_data = "test_macro!(\"Test string.\")\n";

        let found_macros = apply_grammar_to_string(test_data, true);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0]._macro_name(), "test_macro");
        assert_eq!(found_macros[0].position().character(), 12);
        assert_eq!(found_macros[0].position().line(), 1);
        assert_eq!(found_macros[0].position().column(), 13);
        assert_eq!(found_macros[0].reference(), None);
        assert_eq!(
            found_macros[0].insertable_reference_string(123),
            "ref = 123; "
        );
        assert_eq!(found_macros[0].kind(), super::LogRefKind::StructuredNew);
    }

    #[test]
    fn test_grammar_kvp_structured_no_reference_other_kvps()
    {
        let test_data = "test_macro!(host = \"test-1\", os_code = 992; \"Test string.\")\n";

        let found_macros = apply_grammar_to_string(test_data, true);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0]._macro_name(), "test_macro");
        assert_eq!(found_macros[0].position().character(), 12);
        assert_eq!(found_macros[0].position().line(), 1);
        assert_eq!(found_macros[0].position().column(), 13);
        assert_eq!(found_macros[0].reference(), None);
        assert_eq!(
            found_macros[0].insertable_reference_string(123),
            "ref = 123, "
        );
        assert_eq!(found_macros[0].kind(), super::LogRefKind::StructuredNew);
    }

    #[test]
    fn test_grammar_kvp_structured_reference_other_kvps_after_ref()
    {
        let test_data =
            "test_macro!(ref = 123, host = \"test-1\", os_code = 992; \"Test string.\")\n";

        let found_macros = apply_grammar_to_string(test_data, true);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0]._macro_name(), "test_macro");
        assert_eq!(found_macros[0].position().character(), 18);
        assert_eq!(found_macros[0].position().line(), 1);
        assert_eq!(found_macros[0].position().column(), 19);
        assert_eq!(found_macros[0].reference(), Some(123));
        assert_eq!(
            found_macros[0].kind(),
            super::LogRefKind::StructuredPreExisting
        );
    }

    #[test]
    fn test_grammar_kvp_structured_reference_other_kvps_before_ref()
    {
        let test_data =
            "test_macro!(host = \"test-1\", ref = 123, os_code = 992; \"Test string.\")\n";

        let found_macros = apply_grammar_to_string(test_data, true);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0]._macro_name(), "test_macro");
        assert_eq!(found_macros[0].position().character(), 35);
        assert_eq!(found_macros[0].position().line(), 1);
        assert_eq!(found_macros[0].position().column(), 36);
        assert_eq!(found_macros[0].reference(), Some(123));
        assert_eq!(
            found_macros[0].kind(),
            super::LogRefKind::StructuredPreExisting
        );
    }

    #[test]
    fn test_grammar_kvp_structured_reference_last_after_other_kvps()
    {
        let test_data =
            "test_macro!(host = \"test-1\", os_code = 992, ref = 123; \"Test string.\")\n";

        let found_macros = apply_grammar_to_string(test_data, true);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0]._macro_name(), "test_macro");
        assert_eq!(found_macros[0].position().character(), 50);
        assert_eq!(found_macros[0].position().line(), 1);
        assert_eq!(found_macros[0].position().column(), 51);
        assert_eq!(found_macros[0].reference(), Some(123));
        assert_eq!(
            found_macros[0].kind(),
            super::LogRefKind::StructuredPreExisting
        );
    }

    #[test]
    fn test_grammar_kvp_structured_override_extract()
    {
        let test_data = "// breadlog:no-kvp\ntest_macro!(ref = 123; \"[ref: 456] Test string.\")\n";

        let found_macros = apply_grammar_to_string(test_data, true);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0]._macro_name(), "test_macro");
        assert_eq!(found_macros[0].position().character(), 43);
        assert_eq!(found_macros[0].position().line(), 2);
        assert_eq!(found_macros[0].position().column(), 25);
        assert_eq!(found_macros[0].reference(), Some(456));
        assert_eq!(found_macros[0].kind(), super::LogRefKind::String);
    }

    #[test]
    fn test_grammar_kvp_structured_override_insert_with_existing_kvp()
    {
        let test_data = "// breadlog:no-kvp\ntest_macro!(ref = 123; \"Test string.\")\n";

        let found_macros = apply_grammar_to_string(test_data, true);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0]._macro_name(), "test_macro");
        assert_eq!(found_macros[0].position().character(), 43);
        assert_eq!(found_macros[0].position().line(), 2);
        assert_eq!(found_macros[0].position().column(), 25);
        assert_eq!(found_macros[0].reference(), None);
        assert_eq!(found_macros[0].kind(), super::LogRefKind::String);
    }

    #[test]
    fn test_grammar_kvp_structured_override_insert_without_existing_kvp()
    {
        let test_data = "// breadlog:no-kvp\ntest_macro!(\"Test string.\")\n";

        let found_macros = apply_grammar_to_string(test_data, true);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0]._macro_name(), "test_macro");
        assert_eq!(found_macros[0].position().character(), 32);
        assert_eq!(found_macros[0].position().line(), 2);
        assert_eq!(found_macros[0].position().column(), 14);
        assert_eq!(found_macros[0].reference(), None);
        assert_eq!(found_macros[0].kind(), super::LogRefKind::String);
    }
}
