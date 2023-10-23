#![no_main]

use libfuzzer_sys::fuzz_target;
extern crate breadlog;

fuzz_target!(|data: &[u8]| 
{
    if let Ok(fuzz_data) = std::str::from_utf8(data)
    {
        let code_wrapper = format!("fn main() {{\n   log::info(\"{}\");\n}}", fuzz_data);

        let _ = breadlog::parse_rust(code_wrapper.as_str());
    }
});
