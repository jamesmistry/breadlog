use super::CodePosition;
use super::LogRefEntry;
use crate::config::Config;
use pest::Parser;
use std::str::FromStr;

#[derive(Parser)]
#[grammar = "parser/rust_grammar.pest"]
struct RustParser;

pub mod rust_log_ref_finder
{
    use super::*;

    pub fn find(code: &str, config: &Config) -> Vec<LogRefEntry>
    {
        let mut result = Vec::new();

        let parsed_target = RustParser::parse(Rule::file, code).unwrap().next().unwrap();

        for found in parsed_target.into_inner()
        {
            match found.as_rule()
            {
                Rule::log_macro =>
                {
                    let mut inner_rules = found.into_inner();

                    // macro_name
                    let inner_rule = inner_rules.next();

                    let macro_name: &str = match inner_rule
                    {
                        None => continue,
                        Some(rule) => rule.as_str(),
                    };

                    // string_arg
                    let rule_l1 = inner_rules.next();

                    // string_literal
                    let rule_l2 = match rule_l1
                    {
                        None => continue,
                        Some(rule) => rule.into_inner().next(),
                    };

                    // string_value
                    let rule_l3 = match rule_l2
                    {
                        None => continue,
                        Some(rule) => rule.into_inner().next(),
                    };

                    let char_span = match rule_l3
                    {
                        None => continue,
                        Some(rule) => rule.as_span(),
                    };

                    let code_pos = CodePosition::new(
                        char_span.start(),
                        char_span.start_pos().line_col().0,
                        char_span.start_pos().line_col().1,
                    );

                    let macro_name_parsed = String::from_str(macro_name);
                    let macro_name_str = match macro_name_parsed
                    {
                        Err(_e) => continue,
                        Ok(name) => name,
                    };

                    let mut valid_macro = false;
                    for config_macro in &config.rust.log_macros
                    {
                        if macro_name_str == config_macro.name
                        {
                            valid_macro = true;
                            break;
                        }
                        else
                        {
                            let qualified_macro_name =
                                format!("{}::{}", config_macro.module, config_macro.name);

                            if macro_name_str == qualified_macro_name
                            {
                                valid_macro = true;
                                break;
                            }
                        }
                    }

                    if !valid_macro
                    {
                        continue;
                    }

                    let reference =
                        LogRefEntry::extract_reference(&code[char_span.start()..char_span.end()]);

                    let ref_entry = LogRefEntry::new(
                        code_pos,
                        reference,
                        macro_name_str
                            .rfind("::")
                            .map_or(macro_name_str.clone(), |i| {
                                macro_name_str[i + 2..].to_string()
                            }),
                    );

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

    fn create_test_context() -> Context
    {
        let context = Context::new(
            r#"
source_dir: /tmp/test
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
"#
            .to_string(),
            &"/tmp".to_string(),
            false,
        )
        .unwrap();

        context
    }

    fn apply_grammar_to_string(test_data: &str) -> Vec<LogRefEntry>
    {
        let test_data_string = String::from_str(test_data).unwrap();

        let ctx = create_test_context();

        rust_log_ref_finder::find(&test_data_string, &ctx.config)
    }

    #[test]
    fn test_grammar_single_line_literal()
    {
        let test_data = "test_macro!(\"Test string.\")\n";

        let found_macros = apply_grammar_to_string(test_data);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0]._macro_name(), "test_macro");
        assert_eq!(found_macros[0].position().character(), 13);
        assert_eq!(found_macros[0].position().line(), 1);
        assert_eq!(found_macros[0].position().column(), 14);
        assert_eq!(found_macros[0].reference(), None);
    }

    #[test]
    fn test_grammar_multiple_macros()
    {
        let test_data = "test_macro1!(\"Test string 1.\");\ntest_macro2!(\"Test string 2.\");\n";

        let found_macros = apply_grammar_to_string(test_data);

        assert_eq!(found_macros.len(), 2);
        assert_eq!(found_macros[0]._macro_name(), "test_macro1");
        assert_eq!(found_macros[0].position().character(), 14);
        assert_eq!(found_macros[0].position().line(), 1);
        assert_eq!(found_macros[0].position().column(), 15);
        assert_eq!(found_macros[0].reference(), None);
        assert_eq!(found_macros[1]._macro_name(), "test_macro2");
        assert_eq!(found_macros[1].position().character(), 46);
        assert_eq!(found_macros[1].position().line(), 2);
        assert_eq!(found_macros[1].position().column(), 15);
        assert_eq!(found_macros[1].reference(), None);
    }

    #[test]
    fn test_grammar_literal_escape_sequence()
    {
        let test_data =
            "test_macro1!(\"Test \\\"string 1.\");\ntest_macro2!(\"Test string 2.\");\n";

        let found_macros = apply_grammar_to_string(test_data);

        assert_eq!(found_macros.len(), 2);
        assert_eq!(found_macros[0]._macro_name(), "test_macro1");
        assert_eq!(found_macros[0].position().character(), 14);
        assert_eq!(found_macros[0].position().line(), 1);
        assert_eq!(found_macros[0].position().column(), 15);
        assert_eq!(found_macros[0].reference(), None);
        assert_eq!(found_macros[1]._macro_name(), "test_macro2");
        assert_eq!(found_macros[1].position().character(), 48);
        assert_eq!(found_macros[1].position().line(), 2);
        assert_eq!(found_macros[1].position().column(), 15);
        assert_eq!(found_macros[1].reference(), None);
    }

    #[test]
    fn test_grammar_ignore_strings()
    {
        let test_data = "\"test_macro1!(\\\"Test string\\\");\"\ntest_macro2!(\"Test string\");\n";

        let found_macros = apply_grammar_to_string(test_data);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0]._macro_name(), "test_macro2");
        assert_eq!(found_macros[0].position().character(), 47);
        assert_eq!(found_macros[0].position().line(), 2);
        assert_eq!(found_macros[0].position().column(), 15);
        assert_eq!(found_macros[0].reference(), None);
    }

    #[test]
    fn test_grammar_recognise_escaped_quotes()
    {
        let test_data = "\"\\\"test_macro!(\\\"Test string\\\");\"\n";

        let found_macros = apply_grammar_to_string(test_data);

        assert!(found_macros.is_empty());
    }

    #[test]
    fn test_grammar_recognise_escaped_quotes_when_preceded_by_escape_char()
    {
        let test_data = "\"\\\\\\\"test_macro!(\\\"Test string\\\");\"\n";

        let found_macros = apply_grammar_to_string(test_data);

        assert!(found_macros.is_empty());
    }

    #[test]
    fn test_grammar_string_precedes_macro()
    {
        let test_data = "\"This is a string.\" test_macro!(\"Test string.\")\n";

        let found_macros = apply_grammar_to_string(test_data);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0]._macro_name(), "test_macro");
        assert_eq!(found_macros[0].position().character(), 33);
        assert_eq!(found_macros[0].position().line(), 1);
        assert_eq!(found_macros[0].position().column(), 34);
        assert_eq!(found_macros[0].reference(), None);
    }

    #[test]
    fn test_grammar_string_not_closed_by_escaped_quote()
    {
        let test_data = "\"This is a string.\\\" test_macro!(\\\"Test arg.\\\")\"\n";

        let found_macros = apply_grammar_to_string(test_data);

        assert_eq!(found_macros.len(), 0);
    }

    #[test]
    fn test_grammar_macro_no_args()
    {
        let test_data = "test_macro!()\n";

        let found_macros = apply_grammar_to_string(test_data);

        assert_eq!(found_macros.len(), 0);
    }

    #[test]
    fn test_grammar_macro_no_string_args()
    {
        let test_data = "test_macro!(1234)\n";

        let found_macros = apply_grammar_to_string(test_data);

        assert_eq!(found_macros.len(), 0);
    }

    #[test]
    fn test_grammar_whitespace_before_literal()
    {
        let test_data = "test_macro!(    \"Test.\")\n";

        let found_macros = apply_grammar_to_string(test_data);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0]._macro_name(), "test_macro");
        assert_eq!(found_macros[0].position().character(), 17);
        assert_eq!(found_macros[0].position().line(), 1);
        assert_eq!(found_macros[0].position().column(), 18);
        assert_eq!(found_macros[0].reference(), None);
    }

    #[test]
    fn test_grammar_comment_before_literal()
    {
        let test_data = "test_macro!(/* Test comment. */\"Test.\")\n";

        let found_macros = apply_grammar_to_string(test_data);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0]._macro_name(), "test_macro");
        assert_eq!(found_macros[0].position().character(), 32);
        assert_eq!(found_macros[0].position().line(), 1);
        assert_eq!(found_macros[0].position().column(), 33);
        assert_eq!(found_macros[0].reference(), None);
    }

    #[test]
    fn test_grammar_multiline_comment_ignored()
    {
        let test_data = "/*\ntest_macro!(\"Test.\")\n*/\n";

        let found_macros = apply_grammar_to_string(test_data);

        assert_eq!(found_macros.len(), 0);
    }

    #[test]
    fn test_grammar_single_line_comment_ignored()
    {
        let test_data = "// test_macro!(\"Test.\")\n";

        let found_macros = apply_grammar_to_string(test_data);

        assert_eq!(found_macros.len(), 0);
    }

    #[test]
    fn test_grammar_extract_reference()
    {
        let test_data = "test_macro!(\"[ref: 1234] Test string.\")\n";

        let found_macros = apply_grammar_to_string(test_data);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0].reference(), Some(1234));
    }

    #[test]
    fn test_unconfigured_macro()
    {
        let test_data = "random_macro!(\"[ref: 1234] Test string.\")\n";

        let found_macros = apply_grammar_to_string(test_data);

        assert_eq!(found_macros.len(), 0);
    }

    #[test]
    fn test_qualified_macro()
    {
        let test_data = "test_module::test_macro!(\"[ref: 1234] Test string.\")\n";

        let found_macros = apply_grammar_to_string(test_data);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0].reference(), Some(1234));
        assert_eq!(found_macros[0]._macro_name(), "test_macro");
    }

    #[test]
    fn test_qualified_multipath_macro()
    {
        let test_data = "test_module::test_inner::test_macro3!(\"[ref: 1234] Test string.\")\n";

        let found_macros = apply_grammar_to_string(test_data);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0].reference(), Some(1234));
        assert_eq!(found_macros[0]._macro_name(), "test_macro3");
    }

    #[test]
    fn test_grammar_target_arg()
    {
        let test_data = "test_macro!(target: \"test_target\", \"Test string.\")\n";

        let found_macros = apply_grammar_to_string(test_data);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0]._macro_name(), "test_macro");
        assert_eq!(found_macros[0].position().character(), 36);
        assert_eq!(found_macros[0].position().line(), 1);
        assert_eq!(found_macros[0].position().column(), 37);
        assert_eq!(found_macros[0].reference(), None);
    }

    #[test]
    fn test_grammar_target_arg_multiple_macros()
    {
        let test_data = "test_macro1!(target: \"test_target1\", \"Test string 1.\");\ntest_macro2!(target: \"test_target2\", \"Test string 2.\");\n";

        let found_macros = apply_grammar_to_string(test_data);

        assert_eq!(found_macros.len(), 2);
        assert_eq!(found_macros[0]._macro_name(), "test_macro1");
        assert_eq!(found_macros[0].position().character(), 38);
        assert_eq!(found_macros[0].position().line(), 1);
        assert_eq!(found_macros[0].position().column(), 39);
        assert_eq!(found_macros[0].reference(), None);
        assert_eq!(found_macros[1]._macro_name(), "test_macro2");
        assert_eq!(found_macros[1].position().character(), 94);
        assert_eq!(found_macros[1].position().line(), 2);
        assert_eq!(found_macros[1].position().column(), 39);
        assert_eq!(found_macros[1].reference(), None);
    }

    #[test]
    fn test_grammar_target_arg_extract_reference()
    {
        let test_data = "test_macro!(target: \"test_target1\", \"[ref: 1234] Test string.\")\n";

        let found_macros = apply_grammar_to_string(test_data);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0].reference(), Some(1234));
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

        let found_macros = apply_grammar_to_string(test_data);

        assert_eq!(found_macros.len(), 1);
        assert_eq!(found_macros[0].reference(), None);
    }
}
