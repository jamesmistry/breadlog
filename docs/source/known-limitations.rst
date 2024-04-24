Known Limitations
=================

Breadlog has several known limitations:

- Rust code parsing does not use a full Rust parser, instead conservatively 
  approximating the parsing of code. For example it doesn't take into account 
  modules in scope, instead relying only on identifiers to find log statements.
- Rust code parsing only supports Rust logging macros that follow the 
  semantics of the `log crate <https://crates.io/crates/log>`_, and specifically
  the level-specific macros (``info!``, ``warn!``, ``error!`` and so on but 
  not ``log!``).
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

