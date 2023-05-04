use async_std::task;

use super::CodeFinder;
use crate::parser;
use crate::config::Context;
use log::error;

const START_REFERENCE_ID: u32 = 1;

type MapResult = u32;
pub trait ReferenceProcessor<ReduceResult>
{
    fn map(path: &String, entries: &Vec<parser::LogRefEntry>) -> Option<MapResult>;
    fn reduce(map_result: &Vec<MapResult>) -> Option<ReduceResult>;
}

struct NextReferenceId
{}

impl ReferenceProcessor<u32> for NextReferenceId
{
    fn map(_path: &String, entries: &Vec<parser::LogRefEntry>) -> Option<MapResult>
    {
        use std::cmp;

        let mut max_file_ref: u32 = 0;

        for reference in entries.iter()
        {
            if let Some(reference_id) = reference.reference
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

    fn reduce(map_results: &Vec<MapResult>) -> Option<u32>
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
    match async_std::fs::read_to_string(path).await
    {
        Ok(v) => return Some(v),
        Err(e) =>
        {
            error!("Failed to read file {}: {}", path, e);
            return None;
        }
    };
}

fn process_references<'generator, ProcessorType, ReduceResult>(context: &'generator Context, finder: &'generator CodeFinder) -> Option<ReduceResult>
where
    ProcessorType: ReferenceProcessor<ReduceResult>
{
    let config_task_outer = context.config.clone();
    let stop_flag = context.stop_commanded.clone();
        
    return task::block_on(async
    {
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

            if let Some(map_result) = async_std::task::spawn(async move {
                if let Some(file_contents) = load_code(&path).await
                {
                    let references = parser::code_parser::find_references(language, &file_contents, &config_task_inner);

                    return ProcessorType::map(&path, &references);
                }

                None
            }).await
            {
                all_map_results.push(map_result);
            }
        }

        if stop_flag.load(std::sync::atomic::Ordering::Relaxed)
        {
            return None;
        }

        ProcessorType::reduce(&all_map_results)
    });

}

pub fn generate_code<'generator>(context: &'generator Context) -> Result<u32, &'static str>
{
    if let Some(finder) = CodeFinder::new(context)
    {
        let next_reference_id = process_references::<NextReferenceId, u32>(context, &finder).map_or(START_REFERENCE_ID, |id| { id });
    }
    else
    {
        return Err("Code discovery failed");
    }

    Ok(0)
}

#[cfg(test)]
mod tests
{
    use std::str::FromStr;
    use crate::config::Context;
    use crate::parser::code_parser::CodeLanguage;
    use crate::codegen::CodeFile;
    use tempdir::TempDir;
    use std::fs::File;
    use std::io::Write;
    use super::*;

    struct TestRefProcCount
    {}

    impl ReferenceProcessor<u32> for TestRefProcCount
    {
        fn map(path: &String, entries: &Vec<parser::LogRefEntry>) -> Option<MapResult>
        {
            if path.ends_with("_none.rs")
            {
                return None;
            }

            Some(entries.len() as u32)
        }

        fn reduce(map_results: &Vec<MapResult>) -> Option<u32>
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
            format!(r#"
source_dir: {}
rust:
  log_macros:
    - module: test_module
      name: test_macro
"#, source_dir)
            .to_string(),
        check_mode,)
        .unwrap();

        context
    }

    #[test]
    fn test_next_ref_id_map_no_entries()
    {
        const TEST_PATH: &str = "test.rs";
        let test_input: Vec<parser::LogRefEntry> = Vec::new();

        assert_eq!(NextReferenceId::map(&String::from(TEST_PATH), &test_input), None);
    }

    #[test]
    fn test_next_ref_id_map_none_entries()
    {
        const TEST_PATH: &str = "test.rs";
        let mut test_input: Vec<parser::LogRefEntry> = Vec::new();

        {
            let code_pos = parser::CodePosition::new(1, 1, 1);
            let entry = parser::LogRefEntry::new(code_pos, None, String::from_str("test").unwrap());
            test_input.push(entry);
        }

        {
            let code_pos = parser::CodePosition::new(1, 2, 1);
            let entry = parser::LogRefEntry::new(code_pos, None, String::from_str("test").unwrap());
            test_input.push(entry);
        }

        assert_eq!(NextReferenceId::map(&String::from(TEST_PATH), &test_input), None);
    }

    #[test]
    fn test_next_ref_id_map_max_calc()
    {
        const TEST_PATH: &str = "test.rs";
        let mut test_input: Vec<parser::LogRefEntry> = Vec::new();

        {
            let code_pos = parser::CodePosition::new(1, 1, 1);
            let entry = parser::LogRefEntry::new(code_pos, Some(1), String::from_str("test").unwrap());
            test_input.push(entry);
        }

        {
            let code_pos = parser::CodePosition::new(1, 2, 1);
            let entry = parser::LogRefEntry::new(code_pos, Some(3), String::from_str("test").unwrap());
            test_input.push(entry);
        }

        {
            let code_pos = parser::CodePosition::new(1, 2, 1);
            let entry = parser::LogRefEntry::new(code_pos, Some(8), String::from_str("test").unwrap());
            test_input.push(entry);
        }

        assert_eq!(NextReferenceId::map(&String::from(TEST_PATH), &test_input), Some(8));
    }

    #[test]
    fn test_next_ref_id_reduce_no_results()
    {
        let test_input: Vec<MapResult> = Vec::new();
        assert_eq!(NextReferenceId::reduce(&test_input), Some(1));
    }

    #[test]
    fn test_next_ref_id_reduce_default()
    {
        let mut test_input: Vec<MapResult> = Vec::new();

        test_input.push(0);
        test_input.push(0);

        assert_eq!(NextReferenceId::reduce(&test_input), Some(1));
    }

    #[test]
    fn test_next_ref_id_reduce_max()
    {
        let mut test_input: Vec<MapResult> = Vec::new();

        test_input.push(4);
        test_input.push(2);
        test_input.push(1);

        assert_eq!(NextReferenceId::reduce(&test_input), Some(5));
    }

    #[test]
    fn test_process_no_results()
    {
        let temp_dir = TempDir::new("breadlog_test").unwrap();
        let test_context = create_test_context(&temp_dir.path().to_str().unwrap().to_string(), false);

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

        assert_eq!(process_references::<TestRefProcCount, u32>(&test_context, &test_finder), None);
    }

    #[test]
    fn test_process_results()
    {
        let temp_dir = TempDir::new("breadlog_test").unwrap();
        let test_context = create_test_context(&temp_dir.path().to_str().unwrap().to_string(), false);

        {
            let source_file_path = temp_dir
                .path()
                .join("test_file1.rs")
                .to_str()
                .unwrap()
                .to_string();

            let mut source_file = File::create(&source_file_path).unwrap();
            source_file.write_all(br#"
fn test1() {
    test_macro!("Log test.");
}
            "#).unwrap();
        }

        {
            let source_file_path = temp_dir
                .path()
                .join("test_file2.rs")
                .to_str()
                .unwrap()
                .to_string();

            let mut source_file = File::create(&source_file_path).unwrap();
            source_file.write_all(br#"
fn test2() {
    test_macro!("Log test.");
}
            "#).unwrap();
        }

        let test_finder = CodeFinder::new(&test_context).unwrap();

        assert_eq!(process_references::<TestRefProcCount, u32>(&test_context, &test_finder), Some(2));
    }

    #[test]
    fn test_process_partial_results()
    {
        let temp_dir = TempDir::new("breadlog_test").unwrap();
        let test_context = create_test_context(&temp_dir.path().to_str().unwrap().to_string(), false);

        {
            let source_file_path = temp_dir
                .path()
                .join("test_file1_none.rs")
                .to_str()
                .unwrap()
                .to_string();

            let mut source_file = File::create(&source_file_path).unwrap();
            source_file.write_all(br#"
fn test1() {}
            "#).unwrap();
        }

        {
            let source_file_path = temp_dir
                .path()
                .join("test_file2.rs")
                .to_str()
                .unwrap()
                .to_string();

            let mut source_file = File::create(&source_file_path).unwrap();
            source_file.write_all(br#"
fn test2() {
    test_macro!("Log test.");
}
            "#).unwrap();
        }

        let test_finder = CodeFinder::new(&test_context).unwrap();

        assert_eq!(process_references::<TestRefProcCount, u32>(&test_context, &test_finder), Some(1));
    }

    #[test]
    fn test_process_stop_command()
    {
        use std::sync::atomic::Ordering;

        let temp_dir = TempDir::new("breadlog_test").unwrap();
        let test_context = create_test_context(&temp_dir.path().to_str().unwrap().to_string(), false);

        {
            let source_file_path = temp_dir
                .path()
                .join("test_file1.rs")
                .to_str()
                .unwrap()
                .to_string();

            let mut source_file = File::create(&source_file_path).unwrap();
            source_file.write_all(br#"
fn test1() {
    test_macro!("Log test.");
}
            "#).unwrap();
        }

        {
            let source_file_path = temp_dir
                .path()
                .join("test_file2.rs")
                .to_str()
                .unwrap()
                .to_string();

            let mut source_file = File::create(&source_file_path).unwrap();
            source_file.write_all(br#"
fn test2() {
    test_macro!("Log test.");
}
            "#).unwrap();
        }

        let test_finder = CodeFinder::new(&test_context).unwrap();

        test_context.stop_commanded.fetch_or(true, Ordering::Relaxed);

        assert_eq!(process_references::<TestRefProcCount, u32>(&test_context, &test_finder), None);
    }

    #[test]
    fn test_process_next_reference_id()
    {
        let temp_dir = TempDir::new("breadlog_test").unwrap();
        let test_context = create_test_context(&temp_dir.path().to_str().unwrap().to_string(), false);

        {
            let source_file_path = temp_dir
                .path()
                .join("test_file1.rs")
                .to_str()
                .unwrap()
                .to_string();

            let mut source_file = File::create(&source_file_path).unwrap();
            source_file.write_all(br#"
fn test1() {
    test_macro!("[ref: 9] Log test.");
}
            "#).unwrap();
        }

        {
            let source_file_path = temp_dir
                .path()
                .join("test_file2.rs")
                .to_str()
                .unwrap()
                .to_string();

            let mut source_file = File::create(&source_file_path).unwrap();
            source_file.write_all(br#"
fn test2() {
    test_macro!("[ref: 10] Log test.");
}
            "#).unwrap();
        }

        let test_finder = CodeFinder::new(&test_context).unwrap();

        assert_eq!(process_references::<NextReferenceId, u32>(&test_context, &test_finder), Some(11));
    }
}