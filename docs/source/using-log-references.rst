Using Log References
====================

Extracting references from log messages
---------------------------------------

References can be extracted from log messages using the following regular
expression:

.. code-block:: 

   \[ref: ([0-9]{1,10})\]

When matching, the regular expression above results in one match group 
containing the numerical reference in the log message. Valid references 
may be in the range 1 to 4294967295.

The expression is compatible with common PCRE, ECMAScript, Rust, Python, 
Golang, Java 8, C# and many other regular expression implementations.

Whenever possible, you should extract references from log messages before
storing or analysing them. This way your log storage and analysis system can 
store references in a structured field, allowing you to more easily refer 
to them in queries and probably resulting in them using less storage.

Using references to improve log analysis
----------------------------------------

.. note::
    Log query examples in this section are given in SQL to clearly communicate 
    intent in a (mostly) vendor-agnostic way.

Identifying log event types
^^^^^^^^^^^^^^^^^^^^^^^^^^^

Often we use text search patterns to identify application events from log 
messages.

For example, consider the following log message from the 
`Rocket web framework <https://github.com/rwf2/Rocket/tree/v0.5>`_:

.. code-block:: 

   2023-05-02T21:50:19.963Z INFO Bad incoming HTTP request.

We might use a query over the stored logs to count the events this log message 
is referring to:

.. code-block:: sql

   SELECT COUNT(message) WHERE message = 'Bad incoming HTTP request.';

Now imagine the framework developers decide to improve the context within 
their log messages. The same event is now logged with an updated message:

.. code-block:: 

   Bad incoming GET HTTP request.

Great! There's now a bit more useful information in the log message! 
However, this now breaks our log query. Let's update it:

.. code-block:: sql

   SELECT COUNT(message) WHERE message = 'Bad incoming GET HTTP request.';

Hold on... the ``GET`` in the log message looks like it might be interpolated, 
and could potentially be replaced with alternative values (like ``POST`` or 
``PUT``).

We can update our query to accommodate this:

.. code-block:: sql

   SELECT COUNT(message) WHERE message LIKE 'Bad incoming % HTTP request.';

This works, but doing it this way means we have to accept that:

* Changes to log messages - which may happen without notice - might require us 
  to change our log analysis queries, and break our log analysis until we do!

  * In the above example, our original query would produce the wrong results 
    after the log message change.

* Log analysis query complexity and execution cost will vary with log message 
  content.

  * In the above example, we have introduced a wildcard string comparison 
    which will almost certainly incur more cost at query time than a 
    literal string comparison.

* Technical or performance limitations (e.g. with the query language or 
  analysis tool) may introduce ambiguity into our log analysis.

  * In the above example, a different unrelated log message could be confused
    with the one we were trying to count after introducing the wildcard 
    (imagine another log message like ``Bad incoming file over HTTP request.``)

The log references stored in log messages by Breadlog provide a more reliable, 
lower effort alternative for identifying events based on logs. The equivalent 
original log message from Rocket but with a Breadlog reference looks like 
this:

.. code-block:: 

   2023-05-02T21:50:19.963Z INFO [ref: 24] Bad incoming HTTP request.

Our query to count these events is instead:

.. code-block:: sql

   SELECT COUNT(ref_id) WHERE ref_id = 24;
   
After being updated, the log message looks like this:

.. code-block:: 

   2023-05-02T21:50:19.963Z INFO [ref: 24] Bad incoming GET HTTP request.

Note how the numerical reference doesn't change. This means the query we 
used to analyse the logs still works after the change to the log message.

Also, the query works regardless of the interpolated content and without us 
needing any knowledge upfront of the different possible values that could be 
inserted into the log message.

Aggregating event types
^^^^^^^^^^^^^^^^^^^^^^^

As illustrated above, log references make running aggregate queries across 
logs easier. This is important because it's very common to analyse logs this 
way, for example to:

* Produce a histogram of event types.

  .. code-block:: sql

     SELECT ref_id, COUNT(ref_id) GROUP BY ref_id;

* Produce a time series of event types.

  .. code-block:: sql

     SELECT ref_id, EXTRACT(DAY FROM event_time) as event_day GROUP BY ref_id, event_day ORDER BY event_day DESC;

* Identify how event types are distributed by host.

  .. code-block:: sql

     SELECT hostname, ref_id, COUNT(ref_id) AS num_events GROUP BY hostname, ref_id;

By contrast, doing this with queries using log message text means:

* Having to do text processing within the query.
* Having to handle variable portions of the log message (like in the HTTP verb 
  example above).
* Updating the queries when log message text changes.

Sequence analysis
^^^^^^^^^^^^^^^^^

Looking at sequences of events can be useful, and of course is made more 
accurate with reliable event identifiers. For example:

* An investigation into an application fault might reveal that the fault is 
  preceded by a certain sequence of events. If this sequence could be 
  identified automatically, there might be an opportunity to predict future 
  instances of the fault before it occurs.
* Unusual sequences of events might indicate unexpected system behaviour. If a 
  model of normal event sequences could be built and kept up-to-date, sequences
  deviating from this model could be used to trigger additional checks.
