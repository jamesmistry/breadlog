//! Rocket's logging infrastructure.

use std::fmt;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{de, Serialize, Serializer, Deserialize, Deserializer};
use yansi::{Paint, Painted, Condition};

/// Reexport the `log` crate as `private`.
pub use log as private;

// Expose logging macros (hidden) for use by core/contrib codegen.
macro_rules! define_log_macro {
    ($name:ident: $kind:ident, $target:expr, $d:tt) => (
        #[doc(hidden)]
        #[macro_export]
        macro_rules! $name {
            ($d ($t:tt)*) => ($crate::log::private::$kind!(target: $target, $d ($t)*))
        }
    );
    ($name:ident ($indented:ident): $kind:ident, $target:expr, $d:tt) => (
        define_log_macro!($name: $kind, $target, $d);
        define_log_macro!($indented: $kind, concat!($target, "::_"), $d);
    );
    ($kind:ident, $indented:ident) => (
        define_log_macro!($kind: $kind, module_path!(), $);
        define_log_macro!($indented: $kind, concat!(module_path!(), "::_"), $);

        pub use $indented;
    );
}

define_log_macro!(error, error_);
define_log_macro!(warn, warn_);
define_log_macro!(info, info_);
define_log_macro!(debug, debug_);
define_log_macro!(trace, trace_);
define_log_macro!(launch_meta (launch_meta_): info, "rocket::launch", $);
define_log_macro!(launch_info (launch_msg_): warn, "rocket::launch", $);

// `print!` panics when stdout isn't available, but this macro doesn't. See
// SergioBenitez/Rocket#2019 and rust-lang/rust#46016 for more.
//
// Unfortunately, `libtest` captures output by replacing a special sink that
// `print!`, and _only_ `print!`, writes to. Using `write!` directly bypasses
// this sink. As a result, using this better implementation for logging means
// that test log output isn't captured, muddying `cargo test` output.
//
// As a compromise, we only use this better implementation when we're not
// compiled with `debug_assertions` or running tests, so at least tests run in
// debug-mode won't spew output. NOTE: `cfg(test)` alone isn't sufficient: the
// crate is compiled normally for integration tests.
#[cfg(not(any(debug_assertions, test, doctest)))]
macro_rules! write_out {
    ($($arg:tt)*) => ({
        use std::io::{Write, stdout, stderr};
        let _ = write!(stdout(), $($arg)*).or_else(|e| write!(stderr(), "{}", e));
    })
}

#[cfg(any(debug_assertions, test, doctest))]
macro_rules! write_out {
    ($($arg:tt)*) => (print!($($arg)*))
}

#[derive(Debug)]
struct RocketLogger;

/// Defines the maximum level of log messages to show.
#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub enum LogLevel {
    /// Only shows errors and warnings: `"critical"`.
    Critical,
    /// Shows everything except debug and trace information: `"normal"`.
    Normal,
    /// Shows everything: `"debug"`.
    Debug,
    /// Shows nothing: "`"off"`".
    Off,
}

pub trait PaintExt: Sized {
    fn emoji(self) -> Painted<Self>;
}

// Whether a record is a special `launch_{meta,info}!` record.
fn is_launch_record(record: &log::Metadata<'_>) -> bool {
    record.target().contains("rocket::launch")
}

impl log::Log for RocketLogger {
    #[inline(always)]
    fn enabled(&self, record: &log::Metadata<'_>) -> bool {
        match log::max_level().to_level() {
            Some(max) => record.level() <= max || is_launch_record(record),
            None => false
        }
    }

    fn log(&self, record: &log::Record<'_>) {
        // Print nothing if this level isn't enabled and this isn't launch info.
        if !self.enabled(record.metadata()) {
            return;
        }

        // Don't print Hyper, Rustls or r2d2 messages unless debug is enabled.
        let max = log::max_level();
        let from = |path| record.module_path().map_or(false, |m| m.starts_with(path));
        let debug_only = from("hyper") || from("rustls") || from("r2d2");
        if log::LevelFilter::from(LogLevel::Debug) > max && debug_only {
            return;
        }

        // In Rocket, we abuse targets with suffix "_" to indicate indentation.
        let indented = record.target().ends_with('_');
        if indented {
            write_out!("   {} ", ">>".bold());
        }

        // Downgrade a physical launch `warn` to logical `info`.
        let level = is_launch_record(record.metadata())
            .then(|| log::Level::Info)
            .unwrap_or_else(|| record.level());

        match level {
            log::Level::Error if !indented => {
                write_out!("{} {}\n", "Error:".red().bold(), record.args().red().wrap());
            }
            log::Level::Warn if !indented => {
                write_out!("{} {}\n", "Warning:".yellow().bold(), record.args().yellow().wrap());
            }
            log::Level::Info => write_out!("{}\n", record.args().blue().wrap()),
            log::Level::Trace => write_out!("{}\n", record.args().magenta().wrap()),
            log::Level::Warn => write_out!("{}\n", record.args().yellow().wrap()),
            log::Level::Error => write_out!("{}\n", &record.args().red().wrap()),
            log::Level::Debug => {
                write_out!("\n{} ", "-->".blue().bold());
                if let Some(file) = record.file() {
                    write_out!("{}", file.blue());
                }

                if let Some(line) = record.line() {
                    write_out!(":{}\n", line.blue());
                }

                write_out!("\t{}\n", record.args());
            }
        }
    }

    fn flush(&self) {
        // NOOP: We don't buffer any records.
    }
}

pub(crate) fn init_default() {
    crate::log::init(&crate::Config::debug_default())
}

pub(crate) fn init(config: &crate::Config) {
    static ROCKET_LOGGER_SET: AtomicBool = AtomicBool::new(false);

    // Try to initialize Rocket's logger, recording if we succeeded.
    if log::set_boxed_logger(Box::new(RocketLogger)).is_ok() {
        ROCKET_LOGGER_SET.store(true, Ordering::Release);
    }

    // Always disable colors if requested or if the stdout/err aren't TTYs.
    let should_color = config.cli_colors && Condition::stdouterr_are_tty();
    yansi::whenever(Condition::cached(should_color));

    // Set Rocket-logger specific settings only if Rocket's logger is set.
    if ROCKET_LOGGER_SET.load(Ordering::Acquire) {
        log::set_max_level(config.log_level.into());
    }
}

impl From<LogLevel> for log::LevelFilter {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Critical => log::LevelFilter::Warn,
            LogLevel::Normal => log::LevelFilter::Info,
            LogLevel::Debug => log::LevelFilter::Trace,
            LogLevel::Off => log::LevelFilter::Off
        }
    }
}

impl LogLevel {
    fn as_str(&self) -> &str {
        match self {
            LogLevel::Critical => "critical",
            LogLevel::Normal => "normal",
            LogLevel::Debug => "debug",
            LogLevel::Off => "off",
        }
    }
}

impl FromStr for LogLevel {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let level = match &*s.to_ascii_lowercase() {
            "critical" => LogLevel::Critical,
            "normal" => LogLevel::Normal,
            "debug" => LogLevel::Debug,
            "off" => LogLevel::Off,
            _ => return Err("a log level (off, debug, normal, critical)")
        };

        Ok(level)
    }
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl Serialize for LogLevel {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for LogLevel {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let string = String::deserialize(de)?;
        LogLevel::from_str(&string).map_err(|_| de::Error::invalid_value(
            de::Unexpected::Str(&string),
            &figment::error::OneOf( &["critical", "normal", "debug", "off"])
        ))
    }
}

impl PaintExt for &str {
    /// Paint::masked(), but hidden on Windows due to broken output. See #1122.
    fn emoji(self) -> Painted<Self> {
        #[cfg(windows)] { Paint::new("").mask() }
        #[cfg(not(windows))] { Paint::new(self).mask() }
    }
}
