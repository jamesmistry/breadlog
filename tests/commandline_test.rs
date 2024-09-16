use copy_dir;
use lazy_static::lazy_static;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_yaml;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use tempdir::TempDir;
use test_bin;
use walkdir::WalkDir;

/// Represents the position of a change in a line-based file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct LineChange
{
    pub line: usize,
    pub char: usize,
}

/// Represents the expected changes to a file.
type ExpectedChanges = BTreeMap<String, Vec<LineChange>>;

/// Load and return the expected changes from the given YAML file.
///
/// # Arguments
///
/// * `expected_changes_filename` - The path to the YAML file containing the expected changes.
///
fn load_expected_changes(expected_changes_filename: &String) -> ExpectedChanges
{
    let expected_changes_yaml = std::fs::read_to_string(expected_changes_filename).unwrap();
    serde_yaml::from_str::<ExpectedChanges>(&expected_changes_yaml).unwrap()
}

/// Check that the given file has been modified as expected.
///
/// # Arguments
///
/// * `file_path` - The path to the file to check.
/// * `file_contents` - The contents of the file to check.
/// * `expected_line_changes` - The expected changes to the file.
/// * `all_ids` - A mutable reference to a vector containing all reference IDs found in the file, populated by this function.
///
fn check_modified_file(
    file_path: &String,
    file_contents: &String,
    expected_line_changes: &Vec<LineChange>,
    all_ids: &mut Vec<usize>,
)
{
    let mut line_num: usize = 1;
    for line in file_contents.lines()
    {
        for expected_line_change in expected_line_changes
        {
            if expected_line_change.line == line_num
            {
                lazy_static! {
                    static ref LOG_REF_PATTERN: Regex =
                        Regex::new(r"\[ref: ([0-9]{1,10})\]").unwrap();
                }

                let ref_capture = LOG_REF_PATTERN.captures(line).unwrap();
                let ref_match = ref_capture.get(0).unwrap();

                assert_eq!(
                    ref_match.start() + 1, /* Expected indexes are 1-based */
                    expected_line_change.char,
                    "Unexpected reference position in file: {} (actual: {}, expected: {})",
                    file_path,
                    ref_match.start(),
                    expected_line_change.char
                );

                all_ids.push(ref_capture[1].parse::<usize>().unwrap());
            }
        }

        line_num += 1;
    }
}

/// Check that the given reference IDs are contiguous and unique.
///
/// # Arguments
///
/// * `all_ids` - A vector containing all reference IDs found in the file.
///
fn check_ids_contiguous_and_no_duplicates(all_ids: &Vec<usize>)
{
    let mut sorted_ids = all_ids.clone();
    sorted_ids.sort();

    let mut last_id: usize = 0;
    for id in sorted_ids
    {
        assert!(id > 0, "Reference IDs must be greater than zero");
        assert!(id > last_id, "Reference IDs must be unique");
        last_id = id;
    }
}

#[test]
fn test_no_args()
{
    let output = test_bin::get_test_bin("breadlog")
        .status()
        .unwrap()
        .success();

    assert_eq!(output, false);
}

#[test]
fn test_invalid_config()
{
    let temp_dir = TempDir::new("breadlog_test").unwrap();

    let config_filename = temp_dir
        .path()
        .join("config.yaml")
        .to_str()
        .unwrap()
        .to_string();

    {
        let mut config_file = std::fs::File::create(&config_filename).unwrap();
        config_file
            .write_all(
                br#"
/\/ Invalid config /\/
    "#,
            )
            .unwrap();
    }

    let output = test_bin::get_test_bin("breadlog")
        .args(["--config", &config_filename, "--check"])
        .status()
        .unwrap()
        .success();

    assert_eq!(output, false);
}

#[test]
fn test_check()
{
    let temp_dir = TempDir::new("breadlog_test").unwrap();

    copy_dir::copy_dir(
        Path::new("tests/rust_data"),
        Path::new(temp_dir.path()).join("rust_data"),
    )
    .unwrap();

    let config_filename = temp_dir
        .path()
        .join("rust_data/rocket/breadlog.yaml")
        .to_str()
        .unwrap()
        .to_string();

    let output = test_bin::get_test_bin("breadlog")
        .args(["--config", &config_filename, "--check"])
        .output()
        .unwrap();

    assert_eq!(output.status.success(), false);

    let command_stdout = String::from_utf8(output.stdout).unwrap();

    assert!(command_stdout.contains("Total missing references (all files): 45"));
    assert!(command_stdout.contains("Failed: One or more missing references were found"));

    const DATA_PATH_PREFIX: &str = "tests/rust_data";

    for entry in WalkDir::new(Path::new(DATA_PATH_PREFIX).as_os_str())
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        if entry.path().extension().is_none()
        {
            continue;
        }

        if entry.path().extension().unwrap() != "rs"
        {
            continue;
        }

        let relative_path = entry
            .path()
            .canonicalize()
            .unwrap()
            .clone()
            .to_str()
            .unwrap()
            .to_string()
            .chars()
            .skip(
                std::env::current_dir().unwrap().to_str().unwrap().len()
                    + DATA_PATH_PREFIX.len()
                    + 2,
            )
            .collect::<String>();

        let canonical_file_contents = std::fs::read_to_string(entry.path()).unwrap();

        let test_file_path = temp_dir
            .path()
            .join("rust_data")
            .join(relative_path.clone());
        let test_file_contents = std::fs::read_to_string(test_file_path).unwrap();

        assert_eq!(
            canonical_file_contents, test_file_contents,
            "File {} appears to have been modified but should not have been while in check mode",
            relative_path
        );
    }
}

#[test]
fn test_codegen()
{
    let temp_dir = TempDir::new("breadlog_test").unwrap();

    copy_dir::copy_dir(
        Path::new("tests/rust_data"),
        Path::new(temp_dir.path()).join("rust_data"),
    )
    .unwrap();

    let expected_output_filename = temp_dir
        .path()
        .join("rust_data/rocket/breadlog-test-expected.yaml")
        .to_str()
        .unwrap()
        .to_string();

    let expected_changes = load_expected_changes(&expected_output_filename);

    let config_filename = temp_dir
        .path()
        .join("rust_data/rocket/breadlog.yaml")
        .to_str()
        .unwrap()
        .to_string();

    let cache_filename = temp_dir.path().join("rust_data/rocket/Breadlog.lock");

    let output = test_bin::get_test_bin("breadlog")
        .args(["--config", &config_filename])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(cache_filename.exists());

    let command_stdout = String::from_utf8(output.stdout).unwrap();

    assert!(command_stdout.contains("Num. inserted reference(s): 45"));

    const DATA_PATH_PREFIX: &str = "tests/rust_data";

    let mut expected_ref_count: usize = 0;
    let mut all_ids: Vec<usize> = Vec::new();

    for entry in WalkDir::new(Path::new(DATA_PATH_PREFIX).as_os_str())
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        if entry.path().extension().is_none()
        {
            continue;
        }

        if entry.path().extension().unwrap() != "rs"
        {
            continue;
        }

        let relative_path = entry
            .path()
            .canonicalize()
            .unwrap()
            .clone()
            .to_str()
            .unwrap()
            .to_string()
            .chars()
            .skip(
                std::env::current_dir().unwrap().to_str().unwrap().len()
                    + DATA_PATH_PREFIX.len()
                    + 2,
            )
            .collect::<String>();

        let test_file_path = temp_dir
            .path()
            .join("rust_data")
            .join(relative_path.clone());
        let test_file_contents = std::fs::read_to_string(test_file_path).unwrap();

        if !expected_changes.contains_key(&relative_path)
        {
            /*
             * This file is not expected to be modified.
             */

            let canonical_file_contents = std::fs::read_to_string(entry.path()).unwrap();

            assert_eq!(
                canonical_file_contents, test_file_contents,
                "Unexpected file modification: {}",
                relative_path
            );

            continue;
        }

        let expected_line_changes = expected_changes.get(&relative_path).unwrap();

        check_modified_file(
            &relative_path,
            &test_file_contents,
            expected_line_changes,
            &mut all_ids,
        );

        expected_ref_count += expected_line_changes.len();
    }

    assert_eq!(all_ids.len(), expected_ref_count);

    check_ids_contiguous_and_no_duplicates(&all_ids);
}
