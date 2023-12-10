Running Breadlog
================

Breadlog modes
--------------

Breadlog is invoked in one of two modes:

1. **Check mode:** Check mode guarantees that Breadlog will not modify your 
   code, but instead report on the modifications it *would* have made if it 
   was able to modify your code.
2. **Code generation mode:** Code generation mode will modify your code, 
   inserting references in log messages where they are found to be missing.

Suggested workflow
------------------

1. Make changes to your code.
2. Run Breadlog in code generation mode.
3. Commit the changes you and Breadlog have made.
4. Before releasing/pushing changes, run Breadlog in check mode to confirm
   there are no missing log reference IDs.

Consider using 
`git hooks <https://git-scm.com/book/en/v2/Customizing-Git-Git-Hooks>`_ with a 
tool like 
`pre-commit <https://pre-commit.com/>`_ to help automate your use of Breadlog.

Running from within CI
----------------------

It is good practice to run Breadlog in check mode within your continuous 
integration (CI) pipeline so that you can prevent log statements with missing
reference IDs from making their way into production.

You can run Breadlog from within a CI pipeline with two commands.

First, download and install the latest Breadlog release:

.. code-block:: bash

   curl --proto "=https" -LsSf "https://github.com/jamesmistry/breadlog/releases/latest/download/breadlog-package-linux_x86-64.tar.gz" | sudo tar -xz -C /

Second, run Breadlog in check mode. This command will exit with a non-zero code
if missing log reference IDs are found:

.. code-block:: bash

   breadlog -c ./Breadlog.yaml --check

