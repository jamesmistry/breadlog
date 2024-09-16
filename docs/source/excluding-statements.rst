Excluding Statements
====================

If you'd like Breadlog to exclude particular log statements from its analysis, 
add a comment to the line before the statement with the text ``breadlog:ignore``.

For example:

.. code-block:: rust

   // breadlog:ignore
   info!("This log statement will be ignored by Breadlog.");
