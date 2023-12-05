Configuration
=============

Breadlog requires a single YAML configuration file for each code base it is 
used with. This file should be committed to the corresponding source code 
repository.

See the example Breadlog configuration below for a description of the different
configuration options.

.. code-block:: yaml

   ---

   # Required. The location of the source code to process, relative to the 
   # location of the configuration file.
   source_dir: ./src

   # Optional, default = true. If true (default), causes Breadlog to cache 
   # information from scans of the source code to make future scans faster.
   #
   # If enabled, the information is cached in a file called Breadlog.lock in the 
   # same directory as the configuration file. Breadlog.lock, and changes to it,
   # should be committed to the repository.
   use_cache: true

   # Required. Configuration stanza for Rust code.
   rust:

     # Required. Detail about the macros and their containing modules used in your 
     # code for logging. Breadlog assumes use of the log crate 
     # (https://docs.rs/log/latest/log/), and this example configuration specifies
     # the default module and macro names for use with informational, warning and
     # error log messages.
     #
     # However if you use a different crate with the same semantics but different
     # macro/module names or you alias the module or macros, you may want to 
     # customise the names below.
     log_macros:
       - module: log
         name: info
       - module: log
         name: warn
       - module: log
         name: error

