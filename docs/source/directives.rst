Directives
==========

Directives provide ways to modify Breadlog behaviour from within your code 
using comments.

Disable use of structured logging
---------------------------------

When using structured logging there may be some situations where you still
want a log statement to contain a reference in its log message, rather than
as a key-value pair.

In these scenarios, add a comment to the line before the corresponding 
statement with the text ``breadlog:no-kvp``.

For example:

.. code-block:: rust

   // breadlog:no-kvp
   info!("[ref: 123] This log message will contain the reference, even when structured logging is on.");

Ignore log statements
---------------------

If you'd like Breadlog to ignore particular log statements, add a comment to 
the line before the statement with the text ``breadlog:ignore``.

For example:

.. code-block:: rust

   // breadlog:ignore
   info!("This log statement will be ignored by Breadlog.");


