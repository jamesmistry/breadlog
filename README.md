# Breadlog

![CI](https://github.com/jamesmistry/breadlog/actions/workflows/ci.yaml/badge.svg)

For documentation about using Breadlog, see the [User Guide](https://breadlog.readthedocs.io/en/latest/).

## Overview

Breadlog maintains stable, unique references to log messages in your source 
code.

This helps you identify application events from log messages using a numerical 
ID that stays the same even when log message content changes. No brittle or 
complex text parsing required.

## Installing/Upgrading Breadlog

Install the latest version of Breadlog with the following command:

```bash
curl --proto "=https" -LsSf \
   "https://github.com/jamesmistry/breadlog/releases/latest/download/breadlog-package-linux_x86-64.tar.gz" \
   | sudo tar -xz -C /
```

Test your installation by running Breadlog:

```bash
breadlog --version
```

If you'd like to install a specific version of Breadlog, go to the 
[list of Breadlog releases](https://github.com/jamesmistry/breadlog/releases).


See the [User Guide](https://breadlog.readthedocs.io/en/latest/) for how to get started.

## Using Breadlog

See the [User Guide](https://breadlog.readthedocs.io/en/latest/).

## Building Breadlog

> [!NOTE]
> Breadlog only supports Linux x86-64 targets at the moment.

### Prerequisites

Before building Breadlog, you need to:

- Install the Rust compiler toolchain. [Find instructions at rust-lang.org](https://www.rust-lang.org/tools/install).
- Install the Rust nightly toolchain:

  ```bash
  rustup toolchain install nightly
  ```
- (Optional) Install `rustfmt` and `clippy` (used for code formatting and 
  static analysis, respectively):

  ```bash
  rustup update && rustup component add rustfmt clippy
  ```
- (Optional) Install `cargo-fuzz` (used for fuzz testing):

  ```bash
  cargo install cargo-fuzz
  ```

### Building using Cargo

Breadlog currently requires nightly Rust features.

1. Clone the repository and change your working directory:
   
   ```bash
   git clone git@github.com:jamesmistry/breadlog.git && cd breadlog
   ```
2. Make sure the toolchain is up-to-date:

   ```bash
   rustup update nightly
   ```
3. Build using Cargo:

   ```bash
   cargo +nightly build
   ```
4. Find the Breadlog binary in `target/debug/breadlog` or 
   `target/release/breadlog` if creating a release build.

To build the User Guide using Sphinx:

1. Install Sphinx if you haven't already:

   ```bash
   pip3 install -U sphinx sphinx_rtd_theme
   ```
2. From the repository root run the following command where `<OUTPUT DIR>` is 
   the directory to write the generated HTML:

   ```bash
   sphinx-build -M html docs/ <OUTPUT DIR>
   ```

### Running tests

*All commands below are to be run from the repository root.*

- Run the test suite:

  ```bash
  cargo test
  ```
- Start a fuzz test:

  ```bash
  cargo fuzz run fuzz_rust_parser
  ```
- Run the code format check:

  ```bash
  cargo +nightly fmt -- --check --config-path ./
  ```
- Run the linter:

  ```bash
  cargo clippy -- -D warnings
  ```

