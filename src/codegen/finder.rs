use std::option::Option;

use crate::config::Context;

use crate::parser::code_parser::CodeLanguage;
use log::{error, warn};
use std::fs::metadata;
use walkdir::WalkDir;

/// Represents a single code file.
pub struct CodeFile
{
    /// The path to the file.
    pub path: String,

    /// The language contained in the file.
    pub language: CodeLanguage,
}

impl CodeFile
{
    /// Create a new `CodeFile` instance.
    ///
    /// # Arguments
    ///
    /// * `path` - The path to the file.
    /// * `language` - The language contained in the file.
    pub fn new(path: String, language: CodeLanguage) -> CodeFile
    {
        Self { path, language }
    }
}

/// Represents a collection of code files.
pub struct CodeFinder<'ctx>
{
    /// The list of code files.
    pub code_files: Vec<CodeFile>,

    /// A reference to the `Context` instance.
    context: &'ctx Context,
}

impl<'ctx> CodeFinder<'ctx>
{
    /// Create a new `CodeFinder` instance.
    ///
    /// # Arguments
    ///
    /// * `context` - A reference to the `Context` instance.
    pub fn new(context: &'ctx Context) -> Option<CodeFinder<'ctx>>
    {
        let mut result = Self {
            code_files: Vec::new(),
            context,
        };

        if result.find()
        {
            Some(result)
        }
        else
        {
            None
        }
    }

    /// Find all code files in the configured source directory.
    ///
    /// # Returns
    ///
    /// `true` if the search was successful, `false` otherwise.
    pub fn find(&mut self) -> bool
    {
        /*
         * Clear the list of code files.
         */
        self.code_files.clear();

        /*
         * Sanity check that the configured path is a directory. There's a race
         * here but it's tolerable - this check is to provide help to a user
         * who's configured a path to a file instead of a directory.
         */

        let source_dir_metadata = match metadata(&self.context.config.source_dir)
        {
            Ok(metadata) => metadata,
            Err(_e) =>
            {
                error!("Failed to read source directory metadata");
                return false;
            },
        };

        if !source_dir_metadata.is_dir()
        {
            error!("Configured source path is not a directory");
            return false;
        }

        for entry in WalkDir::new(&self.context.config.source_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            use std::sync::atomic;

            if self.context.stop_commanded.load(atomic::Ordering::Relaxed)
            {
                return false;
            }

            if let Some(extension) = entry.path().extension()
            {
                let extension_str = match extension
                    .to_str()
                    .ok_or("Failed to convert extension to string")
                {
                    Ok(extension) => extension.to_string(),
                    Err(_e) =>
                    {
                        warn!("{}", _e);
                        continue;
                    },
                };

                /*
                 * For now, we only support Rust so only have to check these extensions. In the
                 * future this may be extended to check for multiple configured languages.
                 */
                if self.context.config.rust.extensions.contains(&extension_str)
                {
                    let path_str = match entry.path().to_str()
                    {
                        Some(path) => path.to_string(),
                        None => continue,
                    };

                    self.code_files
                        .push(CodeFile::new(path_str, CodeLanguage::Rust));
                }
            }
        }

        true
    }
}

#[cfg(test)]
mod tests
{
    extern crate tempdir;

    use super::*;
    use std::fs::create_dir;
    use std::fs::File;
    use std::io::Write;
    use tempdir::TempDir;

    fn create_test_context(source_dir: String) -> Context
    {
        let test_config = format!(
            r#"
source_dir: {}
rust:
    log_macros:
    - module: test_module
      name: test_macro
"#,
            source_dir
        );

        Context::new(test_config, &"/tmp".to_string(), false).unwrap()
    }

    fn search_codefile(needle: &String, haystack: &Vec<CodeFile>) -> bool
    {
        for code_file in haystack
        {
            if code_file.path == *needle
            {
                return true;
            }
        }

        false
    }

    #[test]
    fn test_find_non_existent_source_dir()
    {
        let temp_dir = TempDir::new("breadlog_test").unwrap();
        let source_dir_path = temp_dir
            .path()
            .join("non_existant")
            .to_str()
            .unwrap()
            .to_string();

        let context = create_test_context(source_dir_path);

        let finder = CodeFinder::new(&context);
        assert!(finder.is_none());
    }

    #[test]
    fn test_find_source_dir_is_file()
    {
        let temp_dir = TempDir::new("breadlog_test").unwrap();
        let source_file_path = temp_dir
            .path()
            .join("test_file")
            .to_str()
            .unwrap()
            .to_string();
        let mut source_file = File::create(&source_file_path).unwrap();
        source_file.write_all(b"Test file").unwrap();

        let context = create_test_context(source_file_path);

        let finder = CodeFinder::new(&context);
        assert!(finder.is_none());
    }

    #[test]
    fn test_no_matching_extensions()
    {
        let temp_dir = TempDir::new("breadlog_test").unwrap();

        {
            let source_file_path = temp_dir
                .path()
                .join("test_file1.c")
                .to_str()
                .unwrap()
                .to_string();

            let mut source_file = File::create(&source_file_path).unwrap();
            source_file.write_all(b"Test file").unwrap();
        }

        {
            let source_file_path = temp_dir
                .path()
                .join("test_file2.cpp")
                .to_str()
                .unwrap()
                .to_string();

            let mut source_file = File::create(&source_file_path).unwrap();
            source_file.write_all(b"Test file").unwrap();
        }

        {
            let source_file_path = temp_dir
                .path()
                .join("test_file3.py")
                .to_str()
                .unwrap()
                .to_string();

            let mut source_file = File::create(&source_file_path).unwrap();
            source_file.write_all(b"Test file").unwrap();
        }

        let mut context = create_test_context(temp_dir.path().to_str().unwrap().to_string());
        context.config.rust.extensions.clear();
        context.config.rust.extensions.push("rs".to_string());

        let finder = CodeFinder::new(&context).unwrap();
        assert_eq!(finder.code_files.len(), 0);
    }

    #[test]
    fn test_no_extensions()
    {
        let temp_dir = TempDir::new("breadlog_test").unwrap();

        {
            let source_file_path = temp_dir
                .path()
                .join("test_file1")
                .to_str()
                .unwrap()
                .to_string();

            let mut source_file = File::create(&source_file_path).unwrap();
            source_file.write_all(b"Test file").unwrap();
        }

        {
            let source_file_path = temp_dir
                .path()
                .join("test_file2")
                .to_str()
                .unwrap()
                .to_string();

            let mut source_file = File::create(&source_file_path).unwrap();
            source_file.write_all(b"Test file").unwrap();
        }

        {
            let source_file_path = temp_dir
                .path()
                .join("test_file3")
                .to_str()
                .unwrap()
                .to_string();

            let mut source_file = File::create(&source_file_path).unwrap();
            source_file.write_all(b"Test file").unwrap();
        }

        let mut context = create_test_context(temp_dir.path().to_str().unwrap().to_string());
        context.config.rust.extensions.clear();
        context.config.rust.extensions.push("rs".to_string());

        let finder = CodeFinder::new(&context).unwrap();
        assert_eq!(finder.code_files.len(), 0);
    }

    #[test]
    fn test_matching_files_single_level()
    {
        let temp_dir = TempDir::new("breadlog_test").unwrap();

        {
            let source_file_path = temp_dir
                .path()
                .join("test_file1.rs")
                .to_str()
                .unwrap()
                .to_string();

            let mut source_file = File::create(&source_file_path).unwrap();
            source_file.write_all(b"Test file").unwrap();
        }

        {
            let source_file_path = temp_dir
                .path()
                .join("test_file2.rs")
                .to_str()
                .unwrap()
                .to_string();

            let mut source_file = File::create(&source_file_path).unwrap();
            source_file.write_all(b"Test file").unwrap();
        }

        {
            let source_file_path = temp_dir
                .path()
                .join("test_file3")
                .to_str()
                .unwrap()
                .to_string();

            let mut source_file = File::create(&source_file_path).unwrap();
            source_file.write_all(b"Test file").unwrap();
        }

        let mut context = create_test_context(temp_dir.path().to_str().unwrap().to_string());
        context.config.rust.extensions.clear();
        context.config.rust.extensions.push("rs".to_string());

        let finder = CodeFinder::new(&context).unwrap();

        assert_eq!(finder.code_files.len(), 2);

        assert!(search_codefile(
            &temp_dir
                .path()
                .join("test_file1.rs")
                .to_str()
                .unwrap()
                .to_string(),
            &finder.code_files
        ));

        assert!(search_codefile(
            &temp_dir
                .path()
                .join("test_file2.rs")
                .to_str()
                .unwrap()
                .to_string(),
            &finder.code_files
        ));

        assert_eq!(finder.code_files[0].language, CodeLanguage::Rust);
        assert_eq!(finder.code_files[1].language, CodeLanguage::Rust);
    }

    #[test]
    fn test_matching_files_multi_level()
    {
        let temp_dir = TempDir::new("breadlog_test").unwrap();
        create_dir(temp_dir.path().join("test_dir1")).unwrap();
        create_dir(temp_dir.path().join("test_dir1").join("test_dir2")).unwrap();

        let nested_path = temp_dir.path().join("test_dir1").join("test_dir2");

        {
            let source_file_path = temp_dir
                .path()
                .join(nested_path.to_str().unwrap().to_string())
                .join("test_file1.rs")
                .to_str()
                .unwrap()
                .to_string();

            let mut source_file = File::create(&source_file_path).unwrap();
            source_file.write_all(b"Test file").unwrap();
        }

        {
            let source_file_path = temp_dir
                .path()
                .join(nested_path.to_str().unwrap().to_string())
                .join("test_file2.rs")
                .to_str()
                .unwrap()
                .to_string();

            let mut source_file = File::create(&source_file_path).unwrap();
            source_file.write_all(b"Test file").unwrap();
        }

        {
            let source_file_path = temp_dir
                .path()
                .join(nested_path.to_str().unwrap().to_string())
                .join("test_file3.py")
                .to_str()
                .unwrap()
                .to_string();

            let mut source_file = File::create(&source_file_path).unwrap();
            source_file.write_all(b"Test file").unwrap();
        }

        let mut context = create_test_context(temp_dir.path().to_str().unwrap().to_string());
        context.config.rust.extensions.clear();
        context.config.rust.extensions.push("rs".to_string());

        let finder = CodeFinder::new(&context).unwrap();
        assert_eq!(finder.code_files.len(), 2);

        assert!(search_codefile(
            &temp_dir
                .path()
                .join(nested_path.to_str().unwrap().to_string())
                .join("test_file1.rs")
                .to_str()
                .unwrap()
                .to_string(),
            &finder.code_files
        ));

        assert!(search_codefile(
            &temp_dir
                .path()
                .join(nested_path.to_str().unwrap().to_string())
                .join("test_file2.rs")
                .to_str()
                .unwrap()
                .to_string(),
            &finder.code_files
        ));

        assert_eq!(finder.code_files[0].language, CodeLanguage::Rust);
        assert_eq!(finder.code_files[1].language, CodeLanguage::Rust);
    }

    #[test]
    fn test_obey_stop_command()
    {
        use std::sync::atomic::Ordering;

        let temp_dir = TempDir::new("breadlog_test").unwrap();

        {
            let source_file_path = temp_dir
                .path()
                .join("test_file1.rs")
                .to_str()
                .unwrap()
                .to_string();

            let mut source_file = File::create(&source_file_path).unwrap();
            source_file.write_all(b"Test file").unwrap();
        }

        let mut context = create_test_context(temp_dir.path().to_str().unwrap().to_string());
        context.config.rust.extensions.clear();
        context.config.rust.extensions.push("rs".to_string());
        context.stop_commanded.fetch_or(true, Ordering::Relaxed);

        let finder = CodeFinder::new(&context);
        assert!(finder.is_none());
    }

    #[test]
    fn test_object_reuse()
    {
        let temp_dir = TempDir::new("breadlog_test").unwrap();

        {
            let source_file_path = temp_dir
                .path()
                .join("test_file1.rs")
                .to_str()
                .unwrap()
                .to_string();

            let mut source_file = File::create(&source_file_path).unwrap();
            source_file.write_all(b"Test file").unwrap();
        }

        {
            let source_file_path = temp_dir
                .path()
                .join("test_file2.rs")
                .to_str()
                .unwrap()
                .to_string();

            let mut source_file = File::create(&source_file_path).unwrap();
            source_file.write_all(b"Test file").unwrap();
        }

        {
            let source_file_path = temp_dir
                .path()
                .join("test_file3")
                .to_str()
                .unwrap()
                .to_string();

            let mut source_file = File::create(&source_file_path).unwrap();
            source_file.write_all(b"Test file").unwrap();
        }

        let mut context = create_test_context(temp_dir.path().to_str().unwrap().to_string());
        context.config.rust.extensions.clear();
        context.config.rust.extensions.push("rs".to_string());

        let mut finder = CodeFinder::new(&context).unwrap();

        assert_eq!(finder.code_files.len(), 2);

        assert!(search_codefile(
            &temp_dir
                .path()
                .join("test_file1.rs")
                .to_str()
                .unwrap()
                .to_string(),
            &finder.code_files
        ));
        assert!(search_codefile(
            &temp_dir
                .path()
                .join("test_file2.rs")
                .to_str()
                .unwrap()
                .to_string(),
            &finder.code_files
        ));

        assert!(finder.find());

        assert_eq!(finder.code_files.len(), 2);

        assert!(search_codefile(
            &temp_dir
                .path()
                .join("test_file1.rs")
                .to_str()
                .unwrap()
                .to_string(),
            &finder.code_files
        ));
        assert!(search_codefile(
            &temp_dir
                .path()
                .join("test_file2.rs")
                .to_str()
                .unwrap()
                .to_string(),
            &finder.code_files
        ));
    }
}
