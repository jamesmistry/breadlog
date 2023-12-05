Known Limitations
=================

Breadlog has several known limitations:

- Rust code parsing does not take into account modules in scope, relying only
  on identifiers to find log statements.
- Rust code parsing only supports Rust logging macros that follow the 
  semantics of the `log crate <https://crates.io/crates/log>`_.
- Rust code parsing does not support `structured key-value pairs 
  <https://docs.rs/log/latest/log/kv/index.html>`_ used with log macros.
- Breadlog does not support specifying check mode exemptions in code (i.e. 
  excluding lines from being checked for the presence of references using 
  in-code annotations).
- The only language Breadlog currently supports is Rust.
- The only platform Breadlog currently supports is Linux x86-64.

If you find a bug, or have a feature request, you can submit the details on `the Breadlog issue tracker 
<https://github.com/jamesmistry/breadlog/issues/new>`_. 
See the `contributing guidelines
<https://github.com/jamesmistry/breadlog/blob/main/CONTRIBUTING.md>`_ for advice
about contributing.

