use async_std::task;
use tempfile::NamedTempFile;

use super::CodeFinder;
use crate::config::Context;
use crate::parser;
use log::error;
use log::warn;
use std::io::Write;
use std::sync::atomic::AtomicU32;
use std::sync::Arc;

const START_REFERENCE_ID: u32 = 1;

pub trait ReferenceProcessor<Params, MapResult, ReduceResult>
{
    fn map(
        path: &str,
        file_contents: &str,
        params: &Option<Params>,
        entries: &[parser::LogRefEntry],
    ) -> Option<MapResult>;
    fn reduce(map_result: &[MapResult]) -> Option<ReduceResult>;
}

struct NextReferenceIdProcessor {}

impl ReferenceProcessor<u32, u32, u32> for NextReferenceIdProcessor
{
    fn map(
        _path: &str,
        _file_contents: &str,
        _params: &Option<u32>,
        entries: &[parser::LogRefEntry],
    ) -> Option<u32>
    {
        use std::cmp;

        let mut max_file_ref: u32 = 0;

        for reference in entries.iter()
        {
            if let Some(reference_id) = reference.reference()
            {
                max_file_ref = cmp::max(max_file_ref, reference_id);
            }
        }

        if max_file_ref > 0
        {
            return Some(max_file_ref);
        }

        None
    }

    fn reduce(map_results: &[u32]) -> Option<u32>
    {
        use std::cmp;

        let mut reduce_result: u32 = 0;

        for map_result in map_results.iter()
        {
            reduce_result = cmp::max(reduce_result, *map_result);
        }

        if reduce_result == 0
        {
            return Some(START_REFERENCE_ID);
        }

        Some(reduce_result + 1)
    }
}

async fn load_code(path: &String) -> Option<String>
{
    return match async_std::fs::read_to_string(path).await
    {
        Ok(v) => Some(v),
        Err(e) =>
        {
            error!("Failed to read file {}: {}", path, e);
            None
        },
    };
}

struct CountMissingReferenceIdProcessor {}

impl ReferenceProcessor<u32, u32, u32> for CountMissingReferenceIdProcessor
{
    fn map(
        path: &str,
        _file_contents: &str,
        _params: &Option<u32>,
        entries: &[parser::LogRefEntry],
    ) -> Option<u32>
    {
        let mut missing_ref_count: u32 = 0;

        for reference in entries.iter()
        {
            if reference.reference() == None
            {
                missing_ref_count += 1;

                warn!(
                    "Missing reference in file {}, line {}, column {}",
                    path,
                    reference.position().line(),
                    reference.position().column(),
                );
            }
        }

        Some(missing_ref_count)
    }

    fn reduce(map_results: &[u32]) -> Option<u32>
    {
        let mut reduce_result: u32 = 0;

        for map_result in map_results.iter()
        {
            reduce_result += *map_result;
        }

        warn!("Total missing references: {}", reduce_result);

        Some(reduce_result)
    }
}

struct InsertReferencesResult
{
    failure: bool,
    num_inserted_references: usize,
}

struct InsertReferencesProcessor {}

impl ReferenceProcessor<Arc<AtomicU32>, InsertReferencesResult, InsertReferencesResult>
    for InsertReferencesProcessor
{
    fn map(
        path: &str,
        file_contents: &str,
        params: &Option<Arc<AtomicU32>>,
        entries: &[parser::LogRefEntry],
    ) -> Option<InsertReferencesResult>
    {
        let mut created_entries: usize = 0;

        let next_reference_id = match params
        {
            Some(next_id) => next_id,
            None =>
            {
                error!("Unexpected missing next reference ID during reference insert");
                return Some(InsertReferencesResult {
                    failure: true,
                    num_inserted_references: 0,
                });
            },
        };

        /* Create a temporary file to write the new contents to. Once the file is written,
         * it will be moved to the original file.
         */
        let mut temp_file = match NamedTempFile::new()
        {
            Ok(f) => f,
            Err(e) =>
            {
                error!("Failed to create temporary file: {}", e);
                return Some(InsertReferencesResult {
                    failure: true,
                    num_inserted_references: 0,
                });
            },
        };

        let scratch_file = temp_file.as_file_mut();

        let mut unwritten_content_start_pos: usize = 0;

        for entry in entries.iter().filter(|e| !e.exists())
        {
            let insert_pos = entry.position().character();

            if insert_pos < unwritten_content_start_pos
            {
                error!(
                    "Unexpected reference insert position {} before cursor position {}",
                    insert_pos, unwritten_content_start_pos,
                );
                return Some(InsertReferencesResult {
                    failure: true,
                    num_inserted_references: 0,
                });
            }

            match scratch_file
                .write_all(&file_contents.as_bytes()[unwritten_content_start_pos..insert_pos])
            {
                Ok(_) => (),
                Err(e) =>
                {
                    error!("Failed to write to temporary file: {}", e);
                    return Some(InsertReferencesResult {
                        failure: true,
                        num_inserted_references: 0,
                    });
                },
            }

            unwritten_content_start_pos += insert_pos - unwritten_content_start_pos;

            let reference_id = next_reference_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let message_header = format!("[ref: {}] ", reference_id);

            match scratch_file.write_all(message_header.as_bytes())
            {
                Ok(_) => (),
                Err(e) =>
                {
                    error!("Failed to write to temporary file: {}", e);
                    return Some(InsertReferencesResult {
                        failure: true,
                        num_inserted_references: 0,
                    });
                },
            }

            created_entries += 1;
        }

        let end_of_file_index = file_contents.len();
        if unwritten_content_start_pos < end_of_file_index
        {
            match scratch_file.write_all(
                &file_contents.as_bytes()[unwritten_content_start_pos..end_of_file_index],
            )
            {
                Ok(_) => (),
                Err(e) =>
                {
                    error!("Failed to write to temporary file: {}", e);
                    return Some(InsertReferencesResult {
                        failure: true,
                        num_inserted_references: 0,
                    });
                },
            }
        }

        match std::fs::rename(temp_file.path(), path)
        {
            Ok(_) => (),
            Err(e) =>
            {
                error!("Failed to rename temporary file: {}", e);
                return Some(InsertReferencesResult {
                    failure: true,
                    num_inserted_references: created_entries,
                });
            },
        }

        Some(InsertReferencesResult {
            failure: false,
            num_inserted_references: created_entries,
        })
    }

    fn reduce(map_results: &[InsertReferencesResult]) -> Option<InsertReferencesResult>
    {
        let mut insert_count: usize = 0;
        let mut reduce_failure: bool = false;

        for map_result in map_results.iter()
        {
            insert_count += map_result.num_inserted_references;
            reduce_failure |= map_result.failure;
        }

        Some(InsertReferencesResult {
            failure: reduce_failure,
            num_inserted_references: insert_count,
        })
    }
}

fn process_references<
    'generator,
    ProcessorType,
    Param: Send + Clone + 'static,
    MapResult: Send + 'static,
    ReduceResult,
>(
    context: &'generator Context,
    params: Option<Param>,
    finder: &'generator CodeFinder,
) -> Option<ReduceResult>
where
    ProcessorType: ReferenceProcessor<Param, MapResult, ReduceResult>,
{
    let config_task_outer = context.config.clone();
    let stop_flag = context.stop_commanded.clone();

    task::block_on(async {
        let mut all_map_results = Vec::new();

        for file in finder.code_files.iter()
        {
            if stop_flag.load(std::sync::atomic::Ordering::Relaxed)
            {
                return None;
            }

            let path = file.path.clone();
            let language = file.language;
            let config_task_inner = config_task_outer.clone();
            let params_task_inner = params.clone();

            if let Some(map_result) = async_std::task::spawn(async move {
                if let Some(file_contents) = load_code(&path).await
                {
                    let references = parser::code_parser::find_references(
                        language,
                        &file_contents,
                        &config_task_inner,
                    );

                    return ProcessorType::map(
                        &path,
                        &file_contents,
                        &params_task_inner,
                        &references,
                    );
                }

                None
            })
            .await
            {
                all_map_results.push(map_result);
            }
        }

        if stop_flag.load(std::sync::atomic::Ordering::Relaxed)
        {
            return None;
        }

        ProcessorType::reduce(&all_map_results)
    })
}

pub fn check_references<'generator>(context: &'generator Context) -> Result<u32, &'static str>
{
    if let Some(finder) = CodeFinder::new(context)
    {
        let missing_reference_count =
            process_references::<CountMissingReferenceIdProcessor, u32, u32, u32>(
                context, None, &finder,
            )
            .map_or(0, |id| id);

        if missing_reference_count > 0
        {
            return Err("One or more missing references were found");
        }
    }
    else
    {
        return Err("Code discovery error");
    }

    Ok(0)
}

pub fn generate_code<'generator>(context: &'generator Context) -> Result<u32, &'static str>
{
    if let Some(finder) = CodeFinder::new(context)
    {
        let _next_reference_id =
            process_references::<NextReferenceIdProcessor, u32, u32, u32>(context, None, &finder)
                .map_or(START_REFERENCE_ID, |id| id);
    }
    else
    {
        return Err("Code discovery error");
    }

    Ok(0)
}

#[cfg(test)]
mod tests
{

    use log::Level;
    use std::fs::File;
    use std::io::Read;
    use std::io::Write;
    use std::str::FromStr;
    use tempdir::TempDir;
    extern crate testing_logger;

    use super::process_references;
    use super::CountMissingReferenceIdProcessor;
    use super::InsertReferencesProcessor;
    use super::InsertReferencesResult;
    use super::NextReferenceIdProcessor;
    use super::ReferenceProcessor;
    use crate::codegen::CodeFinder;
    use crate::config::Context;
    use crate::parser;

    struct TestRefProcCount {}

    impl ReferenceProcessor<u32, u32, u32> for TestRefProcCount
    {
        fn map(
            path: &str,
            _file_contents: &str,
            _params: &Option<u32>,
            entries: &[parser::LogRefEntry],
        ) -> Option<u32>
        {
            if path.ends_with("_none.rs")
            {
                return None;
            }

            Some(entries.len() as u32)
        }

        fn reduce(map_results: &[u32]) -> Option<u32>
        {
            let mut reduce_result: u32 = 0;

            for map_result in map_results.iter()
            {
                reduce_result += *map_result;
            }

            if reduce_result == 0
            {
                return None;
            }

            Some(reduce_result)
        }
    }

    fn create_test_context(source_dir: &String, check_mode: bool) -> Context
    {
        let context = Context::new(
            format!(
                r#"
source_dir: {}
rust:
  log_macros:
    - module: test_module
      name: test_macro
"#,
                source_dir
            )
            .to_string(),
            &"/tmp".to_string(),
            check_mode,
        )
        .unwrap();

        context
            .stop_commanded
            .store(false, std::sync::atomic::Ordering::Relaxed);

        context
    }

    #[test]
    fn test_next_ref_id_map_no_entries()
    {
        const TEST_PATH: &str = "test.rs";
        let test_contents = String::new();
        let test_entries: Vec<parser::LogRefEntry> = Vec::new();

        assert_eq!(
            NextReferenceIdProcessor::map(
                &String::from(TEST_PATH),
                &test_contents,
                &None,
                &test_entries
            ),
            None
        );
    }

    #[test]
    fn test_next_ref_id_map_none_entries()
    {
        const TEST_PATH: &str = "test.rs";
        let test_contents = String::new();
        let mut test_entries: Vec<parser::LogRefEntry> = Vec::new();

        {
            let code_pos = parser::CodePosition::new(1, 1, 1);
            let entry = parser::LogRefEntry::new(code_pos, None, String::from_str("test").unwrap());
            test_entries.push(entry);
        }

        {
            let code_pos = parser::CodePosition::new(1, 2, 1);
            let entry = parser::LogRefEntry::new(code_pos, None, String::from_str("test").unwrap());
            test_entries.push(entry);
        }

        assert_eq!(
            NextReferenceIdProcessor::map(
                &String::from(TEST_PATH),
                &test_contents,
                &None,
                &test_entries
            ),
            None
        );
    }

    #[test]
    fn test_next_ref_id_map_max_calc()
    {
        const TEST_PATH: &str = "test.rs";
        let test_contents = String::new();
        let mut test_entries: Vec<parser::LogRefEntry> = Vec::new();

        {
            let code_pos = parser::CodePosition::new(1, 1, 1);
            let entry =
                parser::LogRefEntry::new(code_pos, Some(1), String::from_str("test").unwrap());
            test_entries.push(entry);
        }

        {
            let code_pos = parser::CodePosition::new(1, 2, 1);
            let entry =
                parser::LogRefEntry::new(code_pos, Some(3), String::from_str("test").unwrap());
            test_entries.push(entry);
        }

        {
            let code_pos = parser::CodePosition::new(1, 2, 1);
            let entry =
                parser::LogRefEntry::new(code_pos, Some(8), String::from_str("test").unwrap());
            test_entries.push(entry);
        }

        assert_eq!(
            NextReferenceIdProcessor::map(
                &String::from(TEST_PATH),
                &test_contents,
                &None,
                &test_entries
            ),
            Some(8)
        );
    }

    #[test]
    fn test_next_ref_id_reduce_no_results()
    {
        let test_input: Vec<u32> = Vec::new();
        assert_eq!(NextReferenceIdProcessor::reduce(&test_input), Some(1));
    }

    #[test]
    fn test_next_ref_id_reduce_default()
    {
        let mut test_input: Vec<u32> = Vec::new();

        test_input.push(0);
        test_input.push(0);

        assert_eq!(NextReferenceIdProcessor::reduce(&test_input), Some(1));
    }

    #[test]
    fn test_next_ref_id_reduce_max()
    {
        let mut test_input: Vec<u32> = Vec::new();

        test_input.push(4);
        test_input.push(2);
        test_input.push(1);

        assert_eq!(NextReferenceIdProcessor::reduce(&test_input), Some(5));
    }

    #[test]
    fn test_missing_ref_map_no_refs()
    {
        const TEST_PATH: &str = "test.rs";
        let test_contents = String::new();
        let test_entries: Vec<parser::LogRefEntry> = Vec::new();

        assert_eq!(
            CountMissingReferenceIdProcessor::map(
                &String::from(TEST_PATH),
                &test_contents,
                &None,
                &test_entries
            ),
            Some(0)
        );
    }

    #[test]
    fn test_missing_ref_map_no_missing_refs()
    {
        const TEST_PATH: &str = "test.rs";
        let test_contents = String::new();
        let mut test_entries: Vec<parser::LogRefEntry> = Vec::new();

        {
            let code_pos = parser::CodePosition::new(1, 2, 3);
            let entry =
                parser::LogRefEntry::new(code_pos, Some(1), String::from_str("test").unwrap());
            test_entries.push(entry);
        }

        {
            let code_pos = parser::CodePosition::new(4, 5, 6);
            let entry =
                parser::LogRefEntry::new(code_pos, Some(2), String::from_str("test").unwrap());
            test_entries.push(entry);
        }

        assert_eq!(
            CountMissingReferenceIdProcessor::map(
                &String::from(TEST_PATH),
                &test_contents,
                &None,
                &test_entries
            ),
            Some(0)
        );
    }

    #[test]
    fn test_missing_ref_map_missing_refs()
    {
        testing_logger::setup();

        const TEST_PATH: &str = "test.rs";
        let test_contents = String::new();
        let mut test_entries: Vec<parser::LogRefEntry> = Vec::new();

        {
            let code_pos = parser::CodePosition::new(1, 2, 3);
            let entry =
                parser::LogRefEntry::new(code_pos, Some(1), String::from_str("test").unwrap());
            test_entries.push(entry);
        }

        {
            let code_pos = parser::CodePosition::new(4, 5, 6);
            let entry = parser::LogRefEntry::new(code_pos, None, String::from_str("test").unwrap());
            test_entries.push(entry);
        }

        assert_eq!(
            CountMissingReferenceIdProcessor::map(
                &String::from(TEST_PATH),
                &test_contents,
                &None,
                &test_entries
            ),
            Some(1)
        );

        testing_logger::validate(|captured_logs| {
            assert_eq!(captured_logs.len(), 1);
            assert_eq!(
                captured_logs[0].body,
                "Missing reference in file test.rs, line 5, column 6"
            );
            assert_eq!(captured_logs[0].level, Level::Warn);
        });
    }

    #[test]
    fn test_process_no_results()
    {
        let temp_dir = TempDir::new("breadlog_test").unwrap();
        let test_context =
            create_test_context(&temp_dir.path().to_str().unwrap().to_string(), false);

        {
            let source_file_path = temp_dir
                .path()
                .join("test_file1_none.rs")
                .to_str()
                .unwrap()
                .to_string();

            let mut source_file = File::create(&source_file_path).unwrap();
            source_file.write_all(b"fn test1() {}").unwrap();
        }

        {
            let source_file_path = temp_dir
                .path()
                .join("test_file2_none.rs")
                .to_str()
                .unwrap()
                .to_string();

            let mut source_file = File::create(&source_file_path).unwrap();
            source_file.write_all(b"fn test2() {}").unwrap();
        }

        let test_finder = CodeFinder::new(&test_context).unwrap();

        assert_eq!(
            process_references::<TestRefProcCount, u32, u32, u32>(
                &test_context,
                None,
                &test_finder
            ),
            None
        );
    }

    #[test]
    fn test_process_results()
    {
        let temp_dir = TempDir::new("breadlog_test").unwrap();
        let test_context =
            create_test_context(&temp_dir.path().to_str().unwrap().to_string(), false);

        {
            let source_file_path = temp_dir
                .path()
                .join("test_file1.rs")
                .to_str()
                .unwrap()
                .to_string();

            let mut source_file = File::create(&source_file_path).unwrap();
            source_file
                .write_all(
                    br#"
fn test1() {
    test_macro!("Log test.");
}
            "#,
                )
                .unwrap();
        }

        {
            let source_file_path = temp_dir
                .path()
                .join("test_file2.rs")
                .to_str()
                .unwrap()
                .to_string();

            let mut source_file = File::create(&source_file_path).unwrap();
            source_file
                .write_all(
                    br#"
fn test2() {
    test_macro!("Log test.");
}
            "#,
                )
                .unwrap();
        }

        let test_finder = CodeFinder::new(&test_context).unwrap();

        assert_eq!(
            process_references::<TestRefProcCount, u32, u32, u32>(
                &test_context,
                None,
                &test_finder
            ),
            Some(2)
        );
    }

    #[test]
    fn test_process_partial_results()
    {
        let temp_dir = TempDir::new("breadlog_test").unwrap();
        let test_context =
            create_test_context(&temp_dir.path().to_str().unwrap().to_string(), false);

        {
            let source_file_path = temp_dir
                .path()
                .join("test_file1_none.rs")
                .to_str()
                .unwrap()
                .to_string();

            let mut source_file = File::create(&source_file_path).unwrap();
            source_file
                .write_all(
                    br#"
fn test1() {}
            "#,
                )
                .unwrap();
        }

        {
            let source_file_path = temp_dir
                .path()
                .join("test_file2.rs")
                .to_str()
                .unwrap()
                .to_string();

            let mut source_file = File::create(&source_file_path).unwrap();
            source_file
                .write_all(
                    br#"
fn test2() {
    test_macro!("Log test.");
}
            "#,
                )
                .unwrap();
        }

        let test_finder = CodeFinder::new(&test_context).unwrap();

        assert_eq!(
            process_references::<TestRefProcCount, u32, u32, u32>(
                &test_context,
                None,
                &test_finder
            ),
            Some(1)
        );
    }

    #[test]
    fn test_process_stop_command()
    {
        use std::sync::atomic::Ordering;

        let temp_dir = TempDir::new("breadlog_test").unwrap();
        let test_context =
            create_test_context(&temp_dir.path().to_str().unwrap().to_string(), false);

        {
            let source_file_path = temp_dir
                .path()
                .join("test_file1.rs")
                .to_str()
                .unwrap()
                .to_string();

            let mut source_file = File::create(&source_file_path).unwrap();
            source_file
                .write_all(
                    br#"
fn test1() {
    test_macro!("Log test.");
}
            "#,
                )
                .unwrap();
        }

        {
            let source_file_path = temp_dir
                .path()
                .join("test_file2.rs")
                .to_str()
                .unwrap()
                .to_string();

            let mut source_file = File::create(&source_file_path).unwrap();
            source_file
                .write_all(
                    br#"
fn test2() {
    test_macro!("Log test.");
}
            "#,
                )
                .unwrap();
        }

        let test_finder = CodeFinder::new(&test_context).unwrap();

        test_context
            .stop_commanded
            .fetch_or(true, Ordering::Relaxed);

        assert_eq!(
            process_references::<TestRefProcCount, u32, u32, u32>(
                &test_context,
                None,
                &test_finder
            ),
            None
        );
    }

    #[test]
    fn test_process_next_reference_id()
    {
        let temp_dir = TempDir::new("breadlog_test").unwrap();
        let test_context =
            create_test_context(&temp_dir.path().to_str().unwrap().to_string(), false);

        {
            let source_file_path = temp_dir
                .path()
                .join("test_file1.rs")
                .to_str()
                .unwrap()
                .to_string();

            let mut source_file = File::create(&source_file_path).unwrap();
            source_file
                .write_all(
                    br#"
fn test1() {
    test_macro!("[ref: 9] Log test.");
}
            "#,
                )
                .unwrap();
        }

        {
            let source_file_path = temp_dir
                .path()
                .join("test_file2.rs")
                .to_str()
                .unwrap()
                .to_string();

            let mut source_file = File::create(&source_file_path).unwrap();
            source_file
                .write_all(
                    br#"
fn test2() {
    test_macro!("[ref: 10] Log test.");
}
            "#,
                )
                .unwrap();
        }

        let test_finder = CodeFinder::new(&test_context).unwrap();

        assert_eq!(
            process_references::<NextReferenceIdProcessor, u32, u32, u32>(
                &test_context,
                None,
                &test_finder
            ),
            Some(11)
        );
    }

    #[test]
    fn test_process_insert_references_no_files()
    {
        use std::sync::atomic::AtomicU32;
        use std::sync::Arc;

        let temp_dir = TempDir::new("breadlog_test").unwrap();
        let test_context =
            create_test_context(&temp_dir.path().to_str().unwrap().to_string(), false);

        let test_finder = CodeFinder::new(&test_context).unwrap();

        let insert_result = process_references::<
            InsertReferencesProcessor,
            Arc<AtomicU32>,
            InsertReferencesResult,
            InsertReferencesResult,
        >(&test_context, None, &test_finder)
        .unwrap();

        /*
         * Because there are no files, the map() function should not be called.
         * As a result, the processor will not report failure despite the lack of the required parameter.
         */

        assert!(!insert_result.failure);
        assert_eq!(insert_result.num_inserted_references, 0);
    }

    #[test]
    fn test_process_insert_references_missing_next_id()
    {
        let temp_dir = TempDir::new("breadlog_test").unwrap();
        let source_file_path = temp_dir
            .path()
            .join("test_file2.rs")
            .to_str()
            .unwrap()
            .to_string();

        File::create(&source_file_path).unwrap();
        let file_contents = String::new();
        let test_entries: Vec<parser::LogRefEntry> = Vec::new();

        testing_logger::setup();

        let insert_result =
            InsertReferencesProcessor::map(&source_file_path, &file_contents, &None, &test_entries)
                .unwrap();

        assert!(insert_result.failure);
        assert_eq!(insert_result.num_inserted_references, 0);

        testing_logger::validate(|captured_logs| {
            assert_eq!(
                captured_logs
                    .iter()
                    .filter(|&message| {
                        message.body
                            == "Unexpected missing next reference ID during reference insert"
                            && message.level == Level::Error
                    })
                    .count(),
                1
            );
        });
    }

    #[test]
    fn test_process_insert_references_out_of_order_insert_pos()
    {
        use std::sync::atomic::AtomicU32;
        use std::sync::Arc;

        let source_file_contents = String::from_str(
            r#"
fn test1() {
    test_macro!("Log test.");
}
"#,
        )
        .unwrap();

        let temp_dir = TempDir::new("breadlog_test").unwrap();
        let source_file_path = temp_dir
            .path()
            .join("test_file.rs")
            .to_str()
            .unwrap()
            .to_string();

        let mut source_file = File::create(&source_file_path).unwrap();
        source_file
            .write_all(source_file_contents.as_bytes())
            .unwrap();

        let mut test_entries: Vec<parser::LogRefEntry> = Vec::new();

        {
            let code_pos = parser::CodePosition::new(30, 2, 18);
            let entry =
                parser::LogRefEntry::new(code_pos, None, String::from_str("test_macro").unwrap());
            test_entries.push(entry);
        }

        {
            let code_pos = parser::CodePosition::new(29, 2, 17);
            let entry =
                parser::LogRefEntry::new(code_pos, None, String::from_str("test_macro").unwrap());
            test_entries.push(entry);
        }

        testing_logger::setup();

        let ref_value = AtomicU32::new(10);
        let ref_arc = Arc::<AtomicU32>::new(ref_value);

        let insert_result = InsertReferencesProcessor::map(
            &source_file_path,
            &source_file_contents,
            &Some(ref_arc),
            &test_entries,
        )
        .unwrap();

        assert!(insert_result.failure);
        assert_eq!(insert_result.num_inserted_references, 0);

        testing_logger::validate(|captured_logs| {
            assert_eq!(
                captured_logs
                    .iter()
                    .filter(|&message| {
                        message
                            .body
                            .starts_with("Unexpected reference insert position")
                            && message.level == Level::Error
                    })
                    .count(),
                1
            );
        });
    }

    #[test]
    fn test_process_insert_references_empty_file()
    {
        use std::sync::atomic::AtomicU32;
        use std::sync::Arc;

        let source_file_contents = String::new();

        let temp_dir = TempDir::new("breadlog_test").unwrap();
        let source_file_path = temp_dir
            .path()
            .join("test_file.rs")
            .to_str()
            .unwrap()
            .to_string();

        let _ = File::create(&source_file_path).unwrap();

        let test_entries: Vec<parser::LogRefEntry> = Vec::new();

        testing_logger::setup();

        let ref_value = AtomicU32::new(1);
        let ref_arc = Arc::<AtomicU32>::new(ref_value);

        let insert_result = InsertReferencesProcessor::map(
            &source_file_path,
            &source_file_contents,
            &Some(ref_arc),
            &test_entries,
        )
        .unwrap();

        assert_eq!(insert_result.failure, false);
        assert_eq!(insert_result.num_inserted_references, 0);
    }

    #[test]
    fn test_process_insert_references_parent_dir_missing()
    {
        use std::sync::atomic::AtomicU32;
        use std::sync::Arc;

        let temp_dir = TempDir::new("breadlog_test").unwrap();
        let source_file_path = temp_dir
            .path()
            .join("does_not_exist")
            .join("test_file.rs")
            .to_str()
            .unwrap()
            .to_string();

        let source_file_contents = String::from_str(
            r#"
fn test1() {
    test_macro!("Log test.");
}
        "#,
        )
        .unwrap();

        let mut test_entries: Vec<parser::LogRefEntry> = Vec::new();

        {
            let code_pos = parser::CodePosition::new(30, 2, 18);
            let entry =
                parser::LogRefEntry::new(code_pos, None, String::from_str("test_macro").unwrap());
            test_entries.push(entry);
        }

        testing_logger::setup();

        let ref_value = AtomicU32::new(10);
        let ref_arc = Arc::<AtomicU32>::new(ref_value);

        let insert_result = InsertReferencesProcessor::map(
            &source_file_path,
            &source_file_contents,
            &Some(ref_arc),
            &test_entries,
        )
        .unwrap();

        assert!(insert_result.failure);
        assert_eq!(insert_result.num_inserted_references, 1);

        testing_logger::validate(|captured_logs| {
            assert_eq!(
                captured_logs
                    .iter()
                    .filter(|&message| {
                        message.body.starts_with("Failed to rename temporary file:")
                            && message.level == Level::Error
                    })
                    .count(),
                1
            );
        });
    }

    #[test]
    fn test_process_insert_references_reduce_success()
    {
        let mut test_input: Vec<InsertReferencesResult> = Vec::new();

        {
            let result = InsertReferencesResult {
                failure: false,
                num_inserted_references: 2,
            };
            test_input.push(result);
        }

        {
            let result = InsertReferencesResult {
                failure: false,
                num_inserted_references: 3,
            };
            test_input.push(result);
        }

        let insert_result = InsertReferencesProcessor::reduce(&test_input).unwrap();

        assert_eq!(insert_result.failure, false);
        assert_eq!(insert_result.num_inserted_references, 5);
    }

    #[test]
    fn test_process_insert_references_reduce_failure()
    {
        let mut test_input: Vec<InsertReferencesResult> = Vec::new();

        {
            let result = InsertReferencesResult {
                failure: true,
                num_inserted_references: 2,
            };
            test_input.push(result);
        }

        {
            let result = InsertReferencesResult {
                failure: false,
                num_inserted_references: 1,
            };
            test_input.push(result);
        }

        let insert_result = InsertReferencesProcessor::reduce(&test_input).unwrap();

        assert!(insert_result.failure);
        assert_eq!(insert_result.num_inserted_references, 3);
    }

    #[test]
    fn test_process_insert_references_files()
    {
        use std::sync::atomic::AtomicU32;
        use std::sync::Arc;

        let temp_dir = TempDir::new("breadlog_test").unwrap();
        let test_context =
            create_test_context(&temp_dir.path().to_str().unwrap().to_string(), false);

        let source_file_path_1 = temp_dir
            .path()
            .join("test_file1.rs")
            .to_str()
            .unwrap()
            .to_string();

        {
            let mut source_file = File::create(&source_file_path_1).unwrap();
            source_file
                .write_all(
                    br#"
    fn test1_1() {
        test_macro!("Log test 1_1.");
    }

    fn test1_2() {
        test_macro!("Log test 1_2.");
    }"#,
                )
                .unwrap();
        }

        let source_file_path_2 = temp_dir
            .path()
            .join("test_file2.rs")
            .to_str()
            .unwrap()
            .to_string();

        {
            let mut source_file = File::create(&source_file_path_2).unwrap();
            source_file
                .write_all(
                    br#"
    fn test2_1() {
        test_macro!("Log test 2_1.");
    }

    fn test2_2() {
        test_macro!("Log test 2_2.");
    }"#,
                )
                .unwrap();
        }

        let test_finder = CodeFinder::new(&test_context).unwrap();

        let ref_value = AtomicU32::new(2);
        let ref_arc = Arc::<AtomicU32>::new(ref_value);

        let insert_result = process_references::<
            InsertReferencesProcessor,
            Arc<AtomicU32>,
            InsertReferencesResult,
            InsertReferencesResult,
        >(&test_context, Some(ref_arc), &test_finder)
        .unwrap();

        /* Load the source files after they have finished being processed. Note that we can't reuse the file objects from
         * earlier because the descriptors will no longer be valid due to the rename operation performed by the processor.
         */
        let mut source_file_1 = File::open(&source_file_path_1).unwrap();
        let mut post_file_contents_1 = String::new();
        source_file_1
            .read_to_string(&mut post_file_contents_1)
            .unwrap();

        let mut source_file_2 = File::open(&source_file_path_2).unwrap();
        let mut post_file_contents_2 = String::new();
        source_file_2
            .read_to_string(&mut post_file_contents_2)
            .unwrap();

        assert_eq!(insert_result.failure, false);
        assert_eq!(insert_result.num_inserted_references, 4);

        /* The order in which files are processed is not deterministic, so we need to check for both possible results.
         */

        assert!(
            (post_file_contents_1
                == String::from_str(
                    r#"
    fn test1_1() {
        test_macro!("[ref: 4] Log test 1_1.");
    }

    fn test1_2() {
        test_macro!("[ref: 5] Log test 1_2.");
    }"#
                )
                .unwrap())
                || (post_file_contents_1
                    == String::from_str(
                        r#"
    fn test1_1() {
        test_macro!("[ref: 2] Log test 1_1.");
    }

    fn test1_2() {
        test_macro!("[ref: 3] Log test 1_2.");
    }"#
                    )
                    .unwrap())
        );

        assert!(
            (post_file_contents_2
                == String::from_str(
                    r#"
    fn test2_1() {
        test_macro!("[ref: 4] Log test 2_1.");
    }

    fn test2_2() {
        test_macro!("[ref: 5] Log test 2_2.");
    }"#
                )
                .unwrap())
                || (post_file_contents_2
                    == String::from_str(
                        r#"
    fn test2_1() {
        test_macro!("[ref: 2] Log test 2_1.");
    }

    fn test2_2() {
        test_macro!("[ref: 3] Log test 2_2.");
    }"#
                    )
                    .unwrap())
        );
    }

    #[test]
    fn test_process_insert_references_empty_files()
    {
        use std::sync::atomic::AtomicU32;
        use std::sync::Arc;

        let temp_dir = TempDir::new("breadlog_test").unwrap();
        let test_context =
            create_test_context(&temp_dir.path().to_str().unwrap().to_string(), false);

        let source_file_path_1 = temp_dir
            .path()
            .join("test_file1.rs")
            .to_str()
            .unwrap()
            .to_string();

        {
            let _ = File::create(&source_file_path_1).unwrap();
        }

        let source_file_path_2 = temp_dir
            .path()
            .join("test_file2.rs")
            .to_str()
            .unwrap()
            .to_string();

        {
            let _ = File::create(&source_file_path_2).unwrap();
        }

        let test_finder = CodeFinder::new(&test_context).unwrap();

        let ref_value = AtomicU32::new(2);
        let ref_arc = Arc::<AtomicU32>::new(ref_value);

        let insert_result = process_references::<
            InsertReferencesProcessor,
            Arc<AtomicU32>,
            InsertReferencesResult,
            InsertReferencesResult,
        >(&test_context, Some(ref_arc), &test_finder)
        .unwrap();

        /* Load the source files after they have finished being processed. Note that we can't reuse the file objects from
         * earlier because the descriptors will no longer be valid due to the rename operation performed by the processor.
         */
        let mut source_file_1 = File::open(&source_file_path_1).unwrap();
        let mut post_file_contents_1 = String::new();
        source_file_1
            .read_to_string(&mut post_file_contents_1)
            .unwrap();

        let mut source_file_2 = File::open(&source_file_path_2).unwrap();
        let mut post_file_contents_2 = String::new();
        source_file_2
            .read_to_string(&mut post_file_contents_2)
            .unwrap();

        assert_eq!(insert_result.failure, false);
        assert_eq!(insert_result.num_inserted_references, 0);
    }
}
