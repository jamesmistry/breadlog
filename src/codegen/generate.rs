use async_std::task;

use super::CodeFinder;
use crate::config::Context;
use crate::parser;
use async_trait::async_trait;
use log::error;
use log::info;
use log::warn;
use std::str::FromStr;
use std::sync::atomic::AtomicU32;
use std::sync::Arc;
use tracing;

const START_REFERENCE_ID: u32 = 1;

async fn load_code(path: &String) -> Option<String>
{
    match async_std::fs::read_to_string(path).await
    {
        Ok(v) => Some(v),
        Err(e) =>
        {
            let path_copy = path.clone();
            task::spawn(async move {
                error!("Failed to read file {}: {}", path_copy, e);
            })
            .await;

            None
        },
    }
}

struct AsyncTempFile
{
    path: String,
    file: async_std::fs::File,
}

impl AsyncTempFile
{
    pub async fn new() -> Result<AsyncTempFile, String>
    {
        use std::env::temp_dir;
        use uuid::Uuid;

        let mut file_path = temp_dir();
        file_path.push(format!("breadlog-{}.tmp", Uuid::new_v4()));

        let path_result = match file_path.to_str()
        {
            Some(p) => match String::from_str(p)
            {
                Ok(s) => s,
                Err(e) =>
                {
                    return Err(format!(
                        "Failed to convert temporary file path to string: {}",
                        e
                    ))
                },
            },
            None => return Err("Failed to convert temporary file path to string".to_string()),
        };

        match async_std::fs::File::create(file_path).await
        {
            Ok(f) => Ok(AsyncTempFile {
                path: path_result,
                file: f,
            }),
            Err(e) => Err(format!("Failed to create temporary file: {}", e)),
        }
    }

    pub fn path(&self) -> &str
    {
        &self.path
    }

    pub fn file(&mut self) -> &mut async_std::fs::File
    {
        &mut self.file
    }
}

impl Drop for AsyncTempFile
{
    fn drop(&mut self)
    {
        use std::fs::remove_file;

        /* The client may have performed a file operation which means it can't be deleted, so
         * don't worry about errors.
         */
        if remove_file(&self.path).is_ok()
        {}
    }
}

#[async_trait]
pub trait ReferenceProcessor<Params, MapResult, ReduceResult>
{
    async fn map(
        path: &str,
        file_contents: &str,
        params: &Option<Params>,
        entries: &[parser::LogRefEntry],
    ) -> Option<MapResult>;
    fn reduce(map_result: &[MapResult]) -> Option<ReduceResult>;
}

struct NextReferenceIdProcessor {}

#[async_trait]
impl ReferenceProcessor<u32, (u32, usize), (u32, usize)> for NextReferenceIdProcessor
{
    async fn map(
        _path: &str,
        _file_contents: &str,
        _params: &Option<u32>,
        entries: &[parser::LogRefEntry],
    ) -> Option<(u32, usize)>
    {
        use std::cmp;

        let mut max_file_ref: u32 = 0;
        let mut num_missing_refs: usize = 0;

        for reference in entries.iter()
        {
            if let Some(reference_id) = reference.reference()
            {
                max_file_ref = cmp::max(max_file_ref, reference_id);
            }
            else
            {
                num_missing_refs += 1;
            }
        }

        return Some((max_file_ref, num_missing_refs));
    }

    fn reduce(map_results: &[(u32, usize)]) -> Option<(u32, usize)>
    {
        use std::cmp;

        let mut ref_id_result: u32 = 0;
        let mut missing_refs_result: usize = 0;

        for map_result in map_results.iter()
        {
            ref_id_result = cmp::max(ref_id_result, map_result.0);
            missing_refs_result += map_result.1;
        }

        if ref_id_result == 0
        {
            return Some((START_REFERENCE_ID, missing_refs_result));
        }

        Some((ref_id_result + 1, missing_refs_result))
    }
}

struct CountMissingReferenceIdProcessor {}

#[async_trait]
impl ReferenceProcessor<u32, u32, u32> for CountMissingReferenceIdProcessor
{
    async fn map(
        path: &str,
        _file_contents: &str,
        _params: &Option<u32>,
        entries: &[parser::LogRefEntry],
    ) -> Option<u32>
    {
        let mut missing_ref_count: u32 = 0;

        for reference in entries.iter()
        {
            if reference.reference().is_none()
            {
                missing_ref_count += 1;

                let path_copy = path.to_string();
                let line = reference.position().line();
                let column = reference.position().column();

                task::spawn(async move {
                    warn!(
                        "Missing reference in file {}, line {}, column {}",
                        path_copy, line, column,
                    );
                })
                .await;

                tracing::event!(
                    tracing::Level::TRACE,
                    "missing_reference_{}_{}",
                    line,
                    column
                );
            }
        }

        let path_copy = path.to_string();
        task::spawn(async move {
            info!(
                "Total missing references in {}: {}",
                path_copy, missing_ref_count
            );
        })
        .await;

        Some(missing_ref_count)
    }

    fn reduce(map_results: &[u32]) -> Option<u32>
    {
        let mut reduce_result: u32 = 0;

        for map_result in map_results.iter()
        {
            reduce_result += *map_result;
        }

        info!("Total missing references (all files): {}", reduce_result);

        Some(reduce_result)
    }
}

struct InsertReferencesResult
{
    failure: bool,
    num_inserted_references: usize,
}

struct InsertReferencesProcessor {}

#[async_trait]
impl ReferenceProcessor<Arc<AtomicU32>, InsertReferencesResult, InsertReferencesResult>
    for InsertReferencesProcessor
{
    async fn map(
        path: &str,
        file_contents: &str,
        params: &Option<Arc<AtomicU32>>,
        entries: &[parser::LogRefEntry],
    ) -> Option<InsertReferencesResult>
    {
        if entries.iter().filter(|&e| !e.exists()).count() == 0
        {
            return Some(InsertReferencesResult {
                failure: false,
                num_inserted_references: 0,
            });
        }

        let mut created_entries: usize = 0;

        let next_reference_id = match params
        {
            Some(next_id) => next_id,
            None =>
            {
                task::spawn(async {
                    error!("Unexpected missing next reference ID during reference insert");
                })
                .await;

                tracing::event!(tracing::Level::TRACE, "unexpected_next_reference_id");

                return Some(InsertReferencesResult {
                    failure: true,
                    num_inserted_references: 0,
                });
            },
        };

        use async_std::io::WriteExt;

        /* Create a temporary file to write the new contents to. Once the file is written,
         * it will be moved to the original file.
         */
        let mut scratch_file = match AsyncTempFile::new().await
        {
            Ok(f) => f,
            Err(e) =>
            {
                task::spawn(async move {
                    error!("{}", e);
                })
                .await;

                return Some(InsertReferencesResult {
                    failure: true,
                    num_inserted_references: 0,
                });
            },
        };

        let mut unwritten_content_start_pos: usize = 0;

        for entry in entries.iter().filter(|e| !e.exists())
        {
            let insert_pos = entry.position().character();

            if insert_pos < unwritten_content_start_pos
            {
                task::spawn(async move {
                    error!(
                        "Unexpected reference insert position {} before cursor position {}",
                        insert_pos, unwritten_content_start_pos,
                    );
                })
                .await;

                tracing::event!(tracing::Level::TRACE, "unexpected_reference_insert_pos");

                return Some(InsertReferencesResult {
                    failure: true,
                    num_inserted_references: 0,
                });
            }

            match scratch_file
                .file()
                .write_all(&file_contents.as_bytes()[unwritten_content_start_pos..insert_pos])
                .await
            {
                Ok(_) => (),
                Err(e) =>
                {
                    task::spawn(async move {
                        error!("Failed to write to temporary file: {}", e);
                    })
                    .await;

                    return Some(InsertReferencesResult {
                        failure: true,
                        num_inserted_references: 0,
                    });
                },
            }

            unwritten_content_start_pos += insert_pos - unwritten_content_start_pos;

            let reference_id = next_reference_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let message_header = format!("[ref: {}] ", reference_id);

            match scratch_file
                .file()
                .write_all(message_header.as_bytes())
                .await
            {
                Ok(_) => (),
                Err(e) =>
                {
                    task::spawn(async move {
                        error!("Failed to write to temporary file: {}", e);
                    })
                    .await;

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
            match scratch_file
                .file()
                .write_all(
                    &file_contents.as_bytes()[unwritten_content_start_pos..end_of_file_index],
                )
                .await
            {
                Ok(_) => (),
                Err(e) =>
                {
                    task::spawn(async move {
                        error!("Failed to write to temporary file: {}", e);
                    })
                    .await;

                    return Some(InsertReferencesResult {
                        failure: true,
                        num_inserted_references: 0,
                    });
                },
            }
        }

        match async_std::fs::rename(scratch_file.path(), path).await
        {
            Ok(_) =>
            {
                return Some(InsertReferencesResult {
                    failure: false,
                    num_inserted_references: created_entries,
                });
            },
            Err(e) =>
            {
                task::spawn(async move {
                    error!("Failed to rename temporary file: {}", e);
                })
                .await;

                tracing::event!(tracing::Level::TRACE, "failed_to_rename_temp_file");

                return Some(InsertReferencesResult {
                    failure: true,
                    num_inserted_references: created_entries,
                });
            },
        }
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

            if let Some(file_contents) = load_code(&path).await
            {
                let references = parser::code_parser::find_references(
                    language,
                    &file_contents,
                    &config_task_inner,
                );

                if let Some(map_result) =
                    ProcessorType::map(&path, &file_contents, &params_task_inner, &references).await
                {
                    all_map_results.push(map_result);
                }
            }
        }

        if stop_flag.load(std::sync::atomic::Ordering::Relaxed)
        {
            return None;
        }

        ProcessorType::reduce(&all_map_results)
    })
}

pub fn check_references(context: &Context) -> Result<u32, &'static str>
{
    if let Some(finder) = CodeFinder::new(context)
    {
        if finder.code_files.is_empty()
        {
            return Err("No files found");
        }

        info!("Found {} file(s)", finder.code_files.len());

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

pub fn generate_code(context: &Context) -> Result<u32, &'static str>
{
    if let Some(finder) = CodeFinder::new(context)
    {
        if finder.code_files.is_empty()
        {
            return Err("No files found");
        }

        info!("Found {} file(s)", finder.code_files.len());

        let calculated_next_reference_id = match context.cached_next_reference_id
        {
            Some(id) =>
            {
                info!("Using cached next reference ID");

                id
            },
            None =>
            {
                info!("Performing first pass to determine next reference ID");

                let references_id_result = match process_references::<
                    NextReferenceIdProcessor,
                    u32,
                    (u32, usize),
                    (u32, usize),
                >(context, None, &finder)
                {
                    Some(r) => r,
                    None => return Err("Failed to determine next reference ID"),
                };

                if references_id_result.1 == 0
                {
                    info!("No missing references - nothing to do");
                    return Ok(0);
                }

                references_id_result.0
            },
        };

        let next_reference_id = Arc::new(AtomicU32::new(calculated_next_reference_id));

        info!(
            "Next reference ID: {}",
            next_reference_id.load(std::sync::atomic::Ordering::Relaxed)
        );

        let reference_updates = match process_references::<
            InsertReferencesProcessor,
            Arc<AtomicU32>,
            InsertReferencesResult,
            InsertReferencesResult,
        >(context, Some(next_reference_id), &finder)
        {
            Some(r) => r,
            None => return Err("Failed to insert references"),
        };

        let cachable_reference_id =
            calculated_next_reference_id + (reference_updates.num_inserted_references as u32) + 1;
        context.cache_next_reference_id(cachable_reference_id, context.config.config_dir.as_str());

        info!(
            "Num. inserted reference(s): {}",
            reference_updates.num_inserted_references
        );
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
    use async_trait::async_trait;
    use futures::AsyncWriteExt;
    use regex::Regex;
    use std::fs::File;
    use std::io::Read;
    use std::io::Write;
    use std::str::FromStr;
    use tempdir::TempDir;
    extern crate testing_logger;
    use tracing_test::traced_test;

    use super::generate_code;
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

    #[async_trait]
    impl ReferenceProcessor<u32, u32, u32> for TestRefProcCount
    {
        async fn map(
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
            &source_dir,
            check_mode,
        )
        .unwrap();

        context
            .stop_commanded
            .store(false, std::sync::atomic::Ordering::Relaxed);

        context
    }

    #[test_log::test(async_std::test)]
    async fn test_next_ref_id_map_no_entries()
    {
        const TEST_PATH: &str = "test.rs";
        let test_contents = String::new();
        let test_entries: Vec<parser::LogRefEntry> = Vec::new();

        let map_result = NextReferenceIdProcessor::map(
            &String::from(TEST_PATH),
            &test_contents,
            &None,
            &test_entries,
        )
        .await;

        assert_eq!(map_result, Some((0, 0)));
    }

    #[test_log::test(async_std::test)]
    async fn test_next_ref_id_map_none_entries()
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

        let map_result = NextReferenceIdProcessor::map(
            &String::from(TEST_PATH),
            &test_contents,
            &None,
            &test_entries,
        )
        .await;

        assert_eq!(map_result, Some((0, 2)));
    }

    #[test_log::test(async_std::test)]
    async fn test_next_ref_id_map_max_calc()
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

        let map_result = NextReferenceIdProcessor::map(
            &String::from(TEST_PATH),
            &test_contents,
            &None,
            &test_entries,
        )
        .await;

        assert_eq!(map_result, Some((8, 0)));
    }

    #[test_log::test(async_std::test)]
    async fn test_missing_refs_map_count()
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
            let entry =
                parser::LogRefEntry::new(code_pos, Some(3), String::from_str("test").unwrap());
            test_entries.push(entry);
        }

        {
            let code_pos = parser::CodePosition::new(1, 2, 1);
            let entry = parser::LogRefEntry::new(code_pos, None, String::from_str("test").unwrap());
            test_entries.push(entry);
        }

        let map_result = NextReferenceIdProcessor::map(
            &String::from(TEST_PATH),
            &test_contents,
            &None,
            &test_entries,
        )
        .await;

        assert_eq!(map_result, Some((3, 2)));
    }

    #[test]
    fn test_next_ref_id_reduce_no_results()
    {
        let test_input: Vec<(u32, usize)> = Vec::new();
        assert_eq!(NextReferenceIdProcessor::reduce(&test_input), Some((1, 0)));
    }

    #[test]
    fn test_next_ref_id_reduce_default()
    {
        let mut test_input: Vec<(u32, usize)> = Vec::new();

        test_input.push((0, 0));
        test_input.push((0, 0));

        assert_eq!(NextReferenceIdProcessor::reduce(&test_input), Some((1, 0)));
    }

    #[test]
    fn test_next_ref_id_reduce_max()
    {
        let mut test_input: Vec<(u32, usize)> = Vec::new();

        test_input.push((4, 0));
        test_input.push((2, 0));
        test_input.push((1, 0));

        assert_eq!(NextReferenceIdProcessor::reduce(&test_input), Some((5, 0)));
    }

    #[test]
    fn test_next_ref_id_missing_ref_reduce_count()
    {
        let mut test_input: Vec<(u32, usize)> = Vec::new();

        test_input.push((4, 1));
        test_input.push((2, 2));
        test_input.push((1, 4));

        assert_eq!(NextReferenceIdProcessor::reduce(&test_input), Some((5, 7)));
    }

    #[test_log::test(async_std::test)]
    async fn test_missing_ref_map_no_refs()
    {
        const TEST_PATH: &str = "test.rs";
        let test_contents = String::new();
        let test_entries: Vec<parser::LogRefEntry> = Vec::new();

        let map_result = CountMissingReferenceIdProcessor::map(
            &String::from(TEST_PATH),
            &test_contents,
            &None,
            &test_entries,
        )
        .await;

        assert_eq!(map_result, Some(0));
    }

    #[test_log::test(async_std::test)]
    async fn test_missing_ref_map_no_missing_refs()
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

        let map_result = CountMissingReferenceIdProcessor::map(
            &String::from(TEST_PATH),
            &test_contents,
            &None,
            &test_entries,
        )
        .await;

        assert_eq!(map_result, Some(0));
    }

    #[test_log::test(async_std::test)]
    #[traced_test]
    async fn test_missing_ref_map_missing_refs()
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
            let entry = parser::LogRefEntry::new(code_pos, None, String::from_str("test").unwrap());
            test_entries.push(entry);
        }

        let map_result = CountMissingReferenceIdProcessor::map(
            &String::from(TEST_PATH),
            &test_contents,
            &None,
            &test_entries,
        )
        .await;

        assert_eq!(map_result, Some(1));

        logs_assert(|lines: &[&str]| {
            match lines
                .iter()
                .filter(|line| line.contains("missing_reference_5_6"))
                .count()
            {
                1 => Ok(()),
                n => Err(format!("More than 1 matching event: {}", n)),
            }
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
            process_references::<NextReferenceIdProcessor, u32, (u32, usize), (u32, usize)>(
                &test_context,
                None,
                &test_finder
            ),
            Some((11, 0))
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

    #[test_log::test(async_std::test)]
    #[traced_test]
    async fn test_process_insert_references_missing_next_id()
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
        let mut test_entries: Vec<parser::LogRefEntry> = Vec::new();

        {
            let code_pos = parser::CodePosition::new(30, 2, 18);
            let entry =
                parser::LogRefEntry::new(code_pos, None, String::from_str("test_macro").unwrap());
            test_entries.push(entry);
        }

        let insert_result =
            InsertReferencesProcessor::map(&source_file_path, &file_contents, &None, &test_entries)
                .await
                .unwrap();

        assert!(insert_result.failure);
        assert_eq!(insert_result.num_inserted_references, 0);

        assert!(logs_contain("unexpected_next_reference_id"));
    }

    #[test_log::test(async_std::test)]
    #[traced_test]
    async fn test_process_insert_references_out_of_order_insert_pos()
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

        let ref_value = AtomicU32::new(10);
        let ref_arc = Arc::<AtomicU32>::new(ref_value);

        let insert_result = InsertReferencesProcessor::map(
            &source_file_path,
            &source_file_contents,
            &Some(ref_arc),
            &test_entries,
        )
        .await
        .unwrap();

        assert!(insert_result.failure);
        assert_eq!(insert_result.num_inserted_references, 0);

        logs_assert(|lines: &[&str]| {
            match lines
                .iter()
                .filter(|line| line.contains("unexpected_reference_insert_pos"))
                .count()
            {
                1 => Ok(()),
                n => Err(format!("More than 1 matching event: {}", n)),
            }
        });
    }

    #[test_log::test(async_std::test)]
    async fn test_process_insert_references_empty_file()
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

        let ref_value = AtomicU32::new(1);
        let ref_arc = Arc::<AtomicU32>::new(ref_value);

        let insert_result = InsertReferencesProcessor::map(
            &source_file_path,
            &source_file_contents,
            &Some(ref_arc),
            &test_entries,
        )
        .await
        .unwrap();

        assert_eq!(insert_result.failure, false);
        assert_eq!(insert_result.num_inserted_references, 0);
    }

    #[test_log::test(async_std::test)]
    #[traced_test]
    async fn test_process_insert_references_parent_dir_missing()
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

        let ref_value = AtomicU32::new(10);
        let ref_arc = Arc::<AtomicU32>::new(ref_value);

        let insert_result = InsertReferencesProcessor::map(
            &source_file_path,
            &source_file_contents,
            &Some(ref_arc),
            &test_entries,
        )
        .await
        .unwrap();

        assert!(insert_result.failure);
        assert_eq!(insert_result.num_inserted_references, 1);

        logs_assert(|lines: &[&str]| {
            match lines
                .iter()
                .filter(|line| line.contains("failed_to_rename_temp_file"))
                .count()
            {
                1 => Ok(()),
                n => Err(format!("More than 1 matching event: {}", n)),
            }
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

    #[test_log::test(async_std::test)]
    async fn test_create_async_temp_file()
    {
        use AsyncWriteExt;

        let temp_file_path: String;

        {
            let mut temp_file_result = crate::codegen::generate::AsyncTempFile::new()
                .await
                .unwrap();
            assert_eq!(
                async_std::fs::metadata(temp_file_result.path())
                    .await
                    .is_ok(),
                true
            );

            temp_file_path = temp_file_result.path().to_string();

            let buffer = String::from_str("test").unwrap();
            assert_ne!(temp_file_result.path(), "");
            assert!(temp_file_result
                .file()
                .write(buffer.as_bytes())
                .await
                .is_ok());
        }

        assert_eq!(async_std::fs::metadata(temp_file_path).await.is_ok(), false);
    }

    #[test]
    fn test_generate_no_cache()
    {
        let temp_dir = TempDir::new("breadlog_test").unwrap();
        let test_context =
            create_test_context(&temp_dir.path().to_str().unwrap().to_string(), false);

        let source_file_path_1: String;
        let source_file_path_2: String;

        {
            source_file_path_1 = temp_dir
                .path()
                .join("test_file1.rs")
                .to_str()
                .unwrap()
                .to_string();

            let mut source_file = File::create(&source_file_path_1).unwrap();
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
            source_file_path_2 = temp_dir
                .path()
                .join("test_file2.rs")
                .to_str()
                .unwrap()
                .to_string();

            let mut source_file = File::create(&source_file_path_2).unwrap();
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

        assert!(generate_code(&test_context).is_ok());

        let file_1_contents = std::fs::read_to_string(&source_file_path_1).unwrap();
        let file_2_contents = std::fs::read_to_string(&source_file_path_2).unwrap();

        let ref_pattern = Regex::new(r"\[ref: ([0-9]{1,10})\]").unwrap();

        let file_1_match = ref_pattern
            .captures_iter(file_1_contents.as_str())
            .next()
            .unwrap();
        let file_2_match = ref_pattern
            .captures_iter(file_2_contents.as_str())
            .next()
            .unwrap();

        let file_1_id = file_1_match[1].parse::<u32>().unwrap();
        let file_2_id = file_2_match[1].parse::<u32>().unwrap();

        assert!(file_1_id != file_2_id);
        assert!(file_1_id == 1 || file_1_id == 2);
        assert!(file_2_id == 1 || file_2_id == 2);
    }

    #[test]
    fn test_generate_uses_cache()
    {
        let temp_dir = TempDir::new("breadlog_test").unwrap();

        {
            let cache_file_path = temp_dir
                .path()
                .join("Breadlog.lock")
                .to_str()
                .unwrap()
                .to_string();

            let mut cache_file = File::create(&cache_file_path).unwrap();
            cache_file
                .write_all(
                    br#"
---
next_reference_id: 123
            "#,
                )
                .unwrap();
        }

        let test_context =
            create_test_context(&temp_dir.path().to_str().unwrap().to_string(), false);

        let source_file_path_1: String;
        let source_file_path_2: String;

        {
            source_file_path_1 = temp_dir
                .path()
                .join("test_file1.rs")
                .to_str()
                .unwrap()
                .to_string();

            let mut source_file = File::create(&source_file_path_1).unwrap();
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
            source_file_path_2 = temp_dir
                .path()
                .join("test_file2.rs")
                .to_str()
                .unwrap()
                .to_string();

            let mut source_file = File::create(&source_file_path_2).unwrap();
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

        assert!(generate_code(&test_context).is_ok());

        let file_1_contents = std::fs::read_to_string(&source_file_path_1).unwrap();
        let file_2_contents = std::fs::read_to_string(&source_file_path_2).unwrap();

        let ref_pattern = Regex::new(r"\[ref: ([0-9]{1,10})\]").unwrap();

        let file_1_match = ref_pattern
            .captures_iter(file_1_contents.as_str())
            .next()
            .unwrap();
        let file_2_match = ref_pattern
            .captures_iter(file_2_contents.as_str())
            .next()
            .unwrap();

        let file_1_id = file_1_match[1].parse::<u32>().unwrap();
        let file_2_id = file_2_match[1].parse::<u32>().unwrap();

        assert!(file_1_id != file_2_id);
        assert!(file_1_id == 123 || file_1_id == 124);
        assert!(file_2_id == 123 || file_2_id == 124);
    }
}
