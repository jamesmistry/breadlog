Getting Started
===============

Step 1: Installing/Upgrading Breadlog
-------------------------------------

.. note::

   Breadlog only supports Linux on x86-64 at the moment.

Install the latest version of Breadlog with the following command:

.. code-block:: bash

   curl --proto "=https" -LsSf \
      "https://github.com/jamesmistry/breadlog/releases/latest/download/breadlog-package-linux_x86-64.tar.gz" \
      | sudo tar -xz -C /

Test your installation by running Breadlog:

.. code-block:: bash

   breadlog --version

If you'd like to install a specific version of Breadlog, go to the
`list of Breadlog releases <https://github.com/jamesmistry/breadlog/releases>`_.

Step 2: Configuring a repository
--------------------------------

.. warning::
   Ensure your code is backed up (e.g. committed to a git repository) before 
   running Breadlog.

1. Create a file called ``Breadlog.yaml`` in the root of your repository.
2. Set the file contents as follows:

   .. code-block:: yaml

      ---
      source_dir: <RELATIVE SOURCE DIRECTORY>
      rust:
        log_macros:
          - module: log
            name: info
          - module: log
            name: warn
          - module: log
            name: error

   Replace ``<RELATIVE SOURCE DIRECTORY>`` above with the path to the source 
   code you want Breadlog to analyse, relative to the repository root.
   
   For example, if your code was in a directory called ``my_app/``, then you 
   would set ``source_dir`` to ``./my_app``.

          
This configuration assumes you're using the `Rust log crate <https://crates.io/crates/log>`_
for logging in your code.

Step 3: Running Breadlog for the first time
-------------------------------------------

It's good practice to run Breadlog in check mode before allowing it to modify 
your code. Check mode guarantees that Breadlog will not modify your code, but
instead report on the modifications it *would* have made if it was in code
generation mode.

Check mode can also be used to verify that there are no missing references in
log statements across your codebase. This is particularly useful in CI 
pipelines. Breadlog's exit code will be non-zero if it finds missing 
references in check mode.

*All commands below are to be run from the repository root.*

1. Run Breadlog in check mode:

   .. code-block:: bash

      breadlog -c ./Breadlog.yaml --check

   You'll see output similar to the following:

   .. code-block:: 

      2023-11-21T10:34:26.943Z INFO [breadlog] [ref: 22] Reading configuration file: ./Breadlog.yaml
      2023-11-21T10:34:26.943Z INFO [breadlog] [ref: 25] Configuration loaded!
      2023-11-21T10:34:26.943Z INFO [breadlog] [ref: 27] Running in check mode
      2023-11-21T10:34:26.945Z INFO [breadlog::codegen::generate] [ref: 15] Found 280 file(s)
      ...
      2023-11-21T10:34:29.872Z INFO [breadlog::codegen::generate] [ref: 6] Total missing references in ././core/http/src/status.rs: 0
      2023-11-21T10:34:29.880Z INFO [breadlog::codegen::generate] [ref: 6] Total missing references in ././core/http/src/lib.rs: 0
      2023-11-21T10:34:29.921Z WARN [breadlog::codegen::generate] [ref: 5] Missing reference in file ././core/http/src/listener.rs, line 178, column 36
      2023-11-21T10:34:29.921Z WARN [breadlog::codegen::generate] [ref: 5] Missing reference in file ././core/http/src/listener.rs, line 186, column 32
      2023-11-21T10:34:29.921Z WARN [breadlog::codegen::generate] [ref: 5] Missing reference in file ././core/http/src/listener.rs, line 189, column 32
      2023-11-21T10:34:29.921Z INFO [breadlog::codegen::generate] [ref: 6] Total missing references in ././core/http/src/listener.rs: 3
      ...
      2023-11-21T10:34:34.987Z INFO [breadlog::codegen::generate] [ref: 7] Total missing references (all files): 46
      2023-11-21T10:34:34.987Z ERROR [breadlog] [ref: 28] Failed: One or more missing references were found

   The locations Breadlog reports missing references are where it will insert 
   references when run in code generation mode (when you omit the ``--check`` 
   flag).

   If you'd like Breadlog to ignore a particular log statement, add a comment 
   to the line before the statement with the text ``breadlog:ignore``. For
   more details, see :doc:`excluding-statements`.

2. Once you're happy with the output, you can run Breadlog in code generation
   mode (without the ``--check`` flag). This will modify your code, inserting 
   references in log messages where they are found to be missing:

   .. code-block:: bash

      breadlog -c ./Breadlog.yaml

3. Assuming you're happy with the changes Breadlog has made, commit them to 
   your repository along with the ``Breadlog.yaml`` and ``Breadlog.lock`` 
   files.

Next steps
----------

Read the other sections in this user guide (it's not very long!) to learn more 
about configuration options, using Breadlog from CI pipelines, known 
limitations and more.
