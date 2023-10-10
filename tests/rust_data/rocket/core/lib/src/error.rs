//! Types representing various errors that can occur in a Rocket application.

use std::{io, fmt};
use std::sync::{Arc, atomic::{Ordering, AtomicBool}};
use std::error::Error as StdError;

use yansi::Paint;
use figment::Profile;

use crate::{Rocket, Orbit};

/// An error that occurs during launch.
///
/// An `Error` is returned by [`launch()`](Rocket::launch()) when launching an
/// application fails or, more rarely, when the runtime fails after launching.
///
/// # Panics
///
/// A value of this type panics if it is dropped without first being inspected.
/// An _inspection_ occurs when any method is called. For instance, if
/// `println!("Error: {}", e)` is called, where `e: Error`, the `Display::fmt`
/// method being called by `println!` results in `e` being marked as inspected;
/// a subsequent `drop` of the value will _not_ result in a panic. The following
/// snippet illustrates this:
///
/// ```rust
/// # let _ = async {
/// if let Err(error) = rocket::build().launch().await {
///     // This println "inspects" the error.
///     println!("Launch failed! Error: {}", error);
///
///     // This call to drop (explicit here for demonstration) will do nothing.
///     drop(error);
/// }
/// # };
/// ```
///
/// When a value of this type panics, the corresponding error message is pretty
/// printed to the console. The following illustrates this:
///
/// ```rust
/// # let _ = async {
/// let error = rocket::build().launch().await;
///
/// // This call to drop (explicit here for demonstration) will result in
/// // `error` being pretty-printed to the console along with a `panic!`.
/// drop(error);
/// # };
/// ```
///
/// # Usage
///
/// An `Error` value should usually be allowed to `drop` without inspection.
/// There are at least two exceptions:
///
///   1. If you are writing a library or high-level application on-top of
///      Rocket, you likely want to inspect the value before it drops to avoid a
///      Rocket-specific `panic!`. This typically means simply printing the
///      value.
///
///   2. You want to display your own error messages.
pub struct Error {
    handled: AtomicBool,
    kind: ErrorKind
}

/// The kind error that occurred.
///
/// In almost every instance, a launch error occurs because of an I/O error;
/// this is represented by the `Io` variant. A launch error may also occur
/// because of ill-defined routes that lead to collisions or because a fairing
/// encountered an error; these are represented by the `Collision` and
/// `FailedFairing` variants, respectively.
#[derive(Debug)]
#[non_exhaustive]
pub enum ErrorKind {
    /// Binding to the provided address/port failed.
    Bind(io::Error),
    /// An I/O error occurred during launch.
    Io(io::Error),
    /// A valid [`Config`](crate::Config) could not be extracted from the
    /// configured figment.
    Config(figment::Error),
    /// Route collisions were detected.
    Collisions(crate::router::Collisions),
    /// Launch fairing(s) failed.
    FailedFairings(Vec<crate::fairing::Info>),
    /// Sentinels requested abort.
    SentinelAborts(Vec<crate::sentinel::Sentry>),
    /// The configuration profile is not debug but not secret key is configured.
    InsecureSecretKey(Profile),
    /// Shutdown failed.
    Shutdown(
        /// The instance of Rocket that failed to shutdown.
        Arc<Rocket<Orbit>>,
        /// The error that occurred during shutdown, if any.
        Option<Box<dyn StdError + Send + Sync>>
    ),
}

/// An error that occurs when a value was unexpectedly empty.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Empty;

impl From<ErrorKind> for Error {
    fn from(kind: ErrorKind) -> Self {
        Error::new(kind)
    }
}

impl Error {
    #[inline(always)]
    pub(crate) fn new(kind: ErrorKind) -> Error {
        Error { handled: AtomicBool::new(false), kind }
    }

    #[inline(always)]
    pub(crate) fn shutdown<E>(rocket: Arc<Rocket<Orbit>>, error: E) -> Error
        where E: Into<Option<crate::http::hyper::Error>>
    {
        let error = error.into().map(|e| Box::new(e) as Box<dyn StdError + Sync + Send>);
        Error::new(ErrorKind::Shutdown(rocket, error))
    }

    #[inline(always)]
    fn was_handled(&self) -> bool {
        self.handled.load(Ordering::Acquire)
    }

    #[inline(always)]
    fn mark_handled(&self) {
        self.handled.store(true, Ordering::Release)
    }

    /// Retrieve the `kind` of the launch error.
    ///
    /// # Example
    ///
    /// ```rust
    /// use rocket::error::ErrorKind;
    ///
    /// # let _ = async {
    /// if let Err(error) = rocket::build().launch().await {
    ///     match error.kind() {
    ///         ErrorKind::Io(e) => println!("found an i/o launch error: {}", e),
    ///         e => println!("something else happened: {}", e)
    ///     }
    /// }
    /// # };
    /// ```
    #[inline]
    pub fn kind(&self) -> &ErrorKind {
        self.mark_handled();
        &self.kind
    }

    /// Prints the error with color (if enabled) and detail. Returns a string
    /// that indicates the abort condition such as "aborting due to i/o error".
    ///
    /// This function is called on `Drop` to display the error message. By
    /// contrast, the `Display` implementation prints a succinct version of the
    /// error, without detail.
    ///
    /// ```rust
    /// # let _ = async {
    /// if let Err(error) = rocket::build().launch().await {
    ///     let abort = error.pretty_print();
    ///     panic!("{}", abort);
    /// }
    /// # };
    /// ```
    pub fn pretty_print(&self) -> &'static str {
        self.mark_handled();
        match self.kind() {
            ErrorKind::Bind(ref e) => {
                error!("Rocket failed to bind network socket to given address/port.");
                info_!("{}", e);
                "aborting due to socket bind error"
            }
            ErrorKind::Io(ref e) => {
                error!("Rocket failed to launch due to an I/O error.");
                info_!("{}", e);
                "aborting due to i/o error"
            }
            ErrorKind::Collisions(ref collisions) => {
                fn log_collisions<T: fmt::Display>(kind: &str, collisions: &[(T, T)]) {
                    if collisions.is_empty() { return }

                    error!("Rocket failed to launch due to the following {} collisions:", kind);
                    for &(ref a, ref b) in collisions {
                        info_!("{} {} {}", a, "collides with".red().italic(), b)
                    }
                }

                log_collisions("route", &collisions.routes);
                log_collisions("catcher", &collisions.catchers);

                info_!("Note: Route collisions can usually be resolved by ranking routes.");
                "aborting due to detected routing collisions"
            }
            ErrorKind::FailedFairings(ref failures) => {
                error!("Rocket failed to launch due to failing fairings:");
                for fairing in failures {
                    info_!("{}", fairing.name);
                }

                "aborting due to fairing failure(s)"
            }
            ErrorKind::InsecureSecretKey(profile) => {
                error!("secrets enabled in non-debug without `secret_key`");
                info_!("selected profile: {}", profile.primary().bold());
                info_!("disable `secrets` feature or configure a `secret_key`");
                "aborting due to insecure configuration"
            }
            ErrorKind::Config(error) => {
                crate::config::pretty_print_error(error.clone());
                "aborting due to invalid configuration"
            }
            ErrorKind::SentinelAborts(ref failures) => {
                error!("Rocket failed to launch due to aborting sentinels:");
                for sentry in failures {
                    let name = sentry.type_name.primary().bold();
                    let (file, line, col) = sentry.location;
                    info_!("{} ({}:{}:{})", name, file, line, col);
                }

                "aborting due to sentinel-triggered abort(s)"
            }
            ErrorKind::Shutdown(_, error) => {
                error!("Rocket failed to shutdown gracefully.");
                if let Some(e) = error {
                    info_!("{}", e);
                }

                "aborting due to failed shutdown"
            }
        }
    }
}

impl std::error::Error for Error {  }

impl fmt::Display for ErrorKind {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorKind::Bind(e) => write!(f, "binding failed: {}", e),
            ErrorKind::Io(e) => write!(f, "I/O error: {}", e),
            ErrorKind::Collisions(_) => "collisions detected".fmt(f),
            ErrorKind::FailedFairings(_) => "launch fairing(s) failed".fmt(f),
            ErrorKind::InsecureSecretKey(_) => "insecure secret key config".fmt(f),
            ErrorKind::Config(_) => "failed to extract configuration".fmt(f),
            ErrorKind::SentinelAborts(_) => "sentinel(s) aborted".fmt(f),
            ErrorKind::Shutdown(_, Some(e)) => write!(f, "shutdown failed: {}", e),
            ErrorKind::Shutdown(_, None) => "shutdown failed".fmt(f),
        }
    }
}

impl fmt::Debug for Error {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.mark_handled();
        self.kind().fmt(f)
    }
}

impl fmt::Display for Error {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.mark_handled();
        write!(f, "{}", self.kind())
    }
}

impl Drop for Error {
    fn drop(&mut self) {
        // Don't panic if the message has been seen. Don't double-panic.
        if self.was_handled() || std::thread::panicking() {
            return
        }

        panic!("{}", self.pretty_print());
    }
}

impl fmt::Debug for Empty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("empty parameter")
    }
}

impl fmt::Display for Empty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("empty parameter")
    }
}

impl StdError for Empty { }
