//! The debugger: a small HTTP server and the views it serves.
//!
//! Two halves. `views` turns what `inspect` and `trace` know into structs
//! shaped for JSON, where every object pointer travels with its description so
//! the front end never has to understand an OOP. `server` is the control loop:
//! it polls for requests between interpreter steps on the one thread, so
//! nothing here needs a lock and the interpreter needs no change.
//!
//! Everything is keyed the way the memory is: an object by its OOP, a bytecode
//! by its method's OOP and its offset. That is what lets later annotations, a
//! method's source text or the native code a JIT made of it, attach to views
//! that already exist rather than reshape them.

pub mod views;
