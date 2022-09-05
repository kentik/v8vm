# v8vm - embedded v8

v8vm provides a high-level interface for embedding the v8 JavaScript
VM in Rust programs. v8vm executes a JavaScript module in a separate
thread and provides an asynchronous interface for invoking functions
exported by the module, as well as an extension interface allowing
JavaScript code to invoke synchronous and asynchronous Rust code.
