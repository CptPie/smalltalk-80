//! The control loop and the HTTP endpoints it answers.
//!
//! One thread. The loop drains whatever requests have arrived, then advances
//! the interpreter by what the run state asks for, then decides whether to
//! stop. A request is served between two bytecodes, never during one, which is
//! why nothing here needs a lock and why the interpreter needs no knowledge
//! that a debugger exists.
//!
//! Handlers never step. They read a view or they change `run`, and the loop
//! does the stepping on its next pass. That keeps every handler the same shape
//! and makes a new way of running the interpreter a new `RunState` variant
//! rather than a new code path.

use std::{
    any::Any,
    collections::HashSet,
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
};

use tiny_http::{Header, Method, Request, Response, Server};

use crate::{
    debugger::views::{FrameView, frame_view, method_view, object_view, state_view},
    errors::ImageLoadError,
    image::Image,
    inspect::{chain_depth, sender_chain},
    interpreter::Interpreter,
    oop::OOP,
};

/// How long to wait before looking for requests again when nothing is running,
/// so that an idle debugger does not spin a core.
const IDLE_POLL: std::time::Duration = std::time::Duration::from_millis(5);

/// What the loop should do with the interpreter.
///
/// Every variant but `Paused` steps once and then asks `should_pause`, so a
/// stop condition is always checked against the state *after* a bytecode has
/// run. A condition that is already true when the request arrives therefore
/// does not stop the loop immediately: asking to run to the instruction you
/// are sitting on means the next time round, which is what a loop body needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunState {
    /// Not stepping. The loop only serves requests.
    Paused,
    /// Stepping until a breakpoint hits.
    Running,
    /// Step this many more bytecodes, then pause.
    Stepping(usize),
    /// Run until the cycle count reaches this.
    UntilCycle(usize),
    /// Run until the next bytecode to execute is this one, in this method.
    ///
    /// Keyed by the method rather than by the context, so every activation of
    /// the method matches. That is what "run to this line" means in a debugger:
    /// stop the next time it runs, whoever is running it.
    UntilLocation { method: OOP, ip: usize },
    /// Run until the sender chain is no deeper than this.
    ///
    /// Step over is the current depth, which returns control once the call that
    /// was about to happen has finished. Step out is one less, which runs to
    /// the end of the current method.
    UntilDepth { at_most: usize },
}

/// A debugging session: an interpreter, where it has got to, and what it should
/// do next.
pub struct Debugger {
    interpreter: Interpreter,
    /// Bytecodes executed since this session booted the image.
    ///
    /// Session relative, because the image records no count of its own: a
    /// snapshot stores state and not history, and the Bluebook's interpreter
    /// keeps no instruction counter. An image resumed mid-send starts at zero
    /// here while sitting at whatever instruction pointer it was saved with.
    ///
    /// What makes the number worth having is that it is reproducible: the same
    /// image stepped the same number of times lands in the same place, which is
    /// what lets `UntilCycle` work and what lets a wrong answer be reported as
    /// happening around a cycle. Real input and timers will weaken that, and
    /// then it degrades to how long the session has been running. The
    /// coordinates that belong to the image rather than the session are the
    /// location and the sender chain, which is why breakpoints use those.
    cycle: usize,
    run: RunState,
    /// Locations to stop at, as (method pointer, byte offset).
    ///
    /// Checked whenever the loop is running of its own accord, so a breakpoint
    /// interrupts `Running` and the `Until` variants alike, but not a `Stepping`
    /// the user asked for explicitly.
    ///
    /// A recompiled method is a new object with a new pointer, so a breakpoint
    /// in it stops firing. That is what a native debugger does after a reload,
    /// and the fix if it ever matters is to key by class and selector and
    /// resolve at check time.
    breakpoints: HashSet<(u16, usize)>,
    /// What the last panic said, if the interpreter has hit one.
    ///
    /// A primitive that is not written yet panics. Catching it and pausing
    /// leaves the whole sender chain readable, which is the entire reason for
    /// having a debugger before having the primitives.
    last_panic: Option<String>,

    /// Where the image was loaded from.
    ///
    /// Kept so that the session can be started over without restarting the
    /// process: a panic leaves the interpreter inspectable but not resumable,
    /// so reloading is the only honest way back to a state worth running.
    image_path: PathBuf,
}

impl Debugger {
    /// Loads an image and builds a session around it, paused before its first
    /// bytecode.
    ///
    /// The loading happens here rather than in the caller so that the session
    /// knows where its image came from, which is what `reset` needs.
    ///
    /// # Parameters:
    ///     * image_path (PathBuf): The image to debug, as a path this process
    ///       can open
    ///
    /// # Returns:
    ///     * Result<Debugger, ImageLoadError>, a paused session, or why the
    ///       image could not be read
    pub fn new(image_path: PathBuf) -> Result<Self, ImageLoadError> {
        let image = Image::load(image_path.clone())?;
        let memory = Image::parse_into_memory(&image)?;
        let interpreter = Interpreter::from_memory(memory);

        Ok(Debugger {
            interpreter,
            run: RunState::Paused,
            cycle: 0,
            breakpoints: HashSet::new(),
            last_panic: None,
            image_path,
        })
    }

    /// Serves requests and runs the interpreter until the process is killed.
    ///
    /// # Parameters:
    ///     * address (&str): What to bind, e.g. `127.0.0.1:8080`
    //
    // The shape of the loop:
    //   drain every request that has arrived, each one reading a view or
    //     changing self.run;
    //   then act on self.run: sleep when paused, otherwise step once;
    //   then, when stepping was not explicitly asked for, stop if a breakpoint
    //     or the run state's own condition says so.
    pub fn serve(&mut self, address: &str) {
        let server = Server::http(address).expect("The server should start up fine");
        loop {
            while let Ok(Some(request)) = server.try_recv() {
                self.handle(request);
            }
            match self.run {
                RunState::Paused => std::thread::sleep(IDLE_POLL),
                _ => {
                    if self.step_once() {
                        if let RunState::Stepping(remaining) = self.run {
                            self.run = if remaining <= 1 {
                                RunState::Paused
                            } else {
                                RunState::Stepping(remaining - 1)
                            };
                        } else if self.should_pause() {
                            self.run = RunState::Paused
                        }
                    }
                }
            }
        }
    }

    /// Advances the interpreter by one bytecode, catching a panic.
    ///
    /// A panic leaves the interpreter where it stopped rather than unwinding
    /// out of the process, so the chain can still be read. The run state goes
    /// to `Paused` and the message is kept for `/state` to report.
    ///
    /// # Returns:
    ///     * bool, false if the step panicked
    fn step_once(&mut self) -> bool {
        let interpreter = &mut self.interpreter;

        let outcome = catch_unwind(AssertUnwindSafe(|| {
            interpreter.step();
        }));

        self.cycle += 1;

        match outcome {
            Ok(()) => true,
            Err(payload) => {
                self.last_panic = Some(panic_message(&payload));
                self.run = RunState::Paused;
                false
            }
        }
    }

    /// Whether the loop should stop, given the state after a step. It is expected that the Stepping
    /// count is decremented before calling this.
    ///
    /// # Returns:
    ///     * bool, true if the run state should become `Paused`
    fn should_pause(&self) -> bool {
        let interpreter = &self.interpreter;

        if !matches!(self.run, RunState::Stepping(_)) && self.is_breakpoint() {
            return true;
        }

        match self.run {
            RunState::Paused => true,
            RunState::Running => false,
            RunState::Stepping(n) => n == 0,
            RunState::UntilCycle(n) => self.cycle >= n,
            RunState::UntilDepth { at_most } => chain_depth(interpreter) <= at_most,
            RunState::UntilLocation { method, ip } => {
                method == interpreter.method() && ip == interpreter.ip()
            }
        }
    }

    /// Determines whether the current interpreter step is a known breakpoint
    ///
    /// # Returns:
    ///     * bool, true if the interpreter step is a registered breakpoint
    fn is_breakpoint(&self) -> bool {
        self.breakpoints
            .contains(&(self.interpreter.method().value, self.interpreter.ip()))
    }
}

// ====================================
//  Routing
// ====================================

impl Debugger {
    /// Answers one request.
    ///
    /// Reads are built fresh from memory every time, which is what keeps the
    /// views honest about an image that rewrites its own methods: there is no
    /// cached listing to go stale.
    ///
    /// # Parameters:
    ///     * request (Request): The request to answer
    //
    // GET  /                 the page itself
    // GET  /state            StateView, small enough to poll
    // GET  /chain            Vec<FrameView>
    // GET  /method/{oop}     MethodView
    // GET  /object/{oop}     ObjectView
    // POST /step?n=1         Stepping(n)
    // POST /run              Running
    // POST /pause            Paused
    // POST /reset            reload the image, keeping breakpoints
    // POST /until?...        one of the Until variants
    // POST /breakpoint       add or remove a (method, ip)
    fn handle(&mut self, request: Request) {
        let method = request.method().clone();
        let url = request.url().to_string();
        let path = url.split('?').next().unwrap_or("");

        match (&method, path) {
            (Method::Get, "/") => {
                Self::respond_html(request, PAGE);
            }
            (Method::Get, "/state") => {
                self.respond_state(request);
            }
            (Method::Get, "/chain") => {
                let chain: Vec<FrameView> = sender_chain(&self.interpreter)
                    .iter()
                    .map(|frame| frame_view(self.interpreter.memory(), frame))
                    .collect();
                Self::respond_json(request, &chain);
            }
            (Method::Get, p) if p.starts_with("/method/") => {
                match oop_from_path(&url, "/method/") {
                    Some(oop) => {
                        Self::respond_json(request, &method_view(self.interpreter.memory(), oop))
                    }
                    None => Self::respond_error(request, 400, "not an object pointer"),
                }
            }
            (Method::Get, p) if p.starts_with("/object/") => {
                match oop_from_path(&url, "/object/") {
                    Some(oop) => {
                        Self::respond_json(request, &object_view(self.interpreter.memory(), oop))
                    }
                    None => Self::respond_error(request, 400, "not an object"),
                }
            }
            (Method::Post, "/step") => {
                let n = query_parameter(&url, "n")
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(1);
                self.run = RunState::Stepping(n);
                self.respond_state(request);
            }
            (Method::Post, "/run") => {
                self.run = RunState::Running;
                self.respond_state(request);
            }
            (Method::Post, "/pause") => {
                self.run = RunState::Paused;
                self.respond_state(request);
            }
            (Method::Post, "/reset") => match self.reset() {
                Ok(()) => self.respond_state(request),
                Err(error) => Self::respond_error(request, 500, &format!("{:?}", error)),
            },
            _ => Self::respond_error(request, 404, "no such route"),
        }
    }

    /// Sends a value as JSON.
    ///
    /// # Parameters:
    ///     * request (Request): The request to answer
    ///     * value (&T): What to serialise
    fn respond_json<T: serde::Serialize>(request: Request, value: &T) {
        let body = match serde_json::to_string(value) {
            Ok(body) => body,
            Err(error) => return Self::respond_error(request, 500, &error.to_string()),
        };

        let json = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
            .expect("a header written by hand is well formed");

        let _ = request.respond(Response::from_string(body).with_header(json));
    }

    /// Sends a status code and a short message, for a bad path or a bad OOP.
    ///
    /// # Parameters:
    ///     * request (Request): The request to answer
    ///     * status (u16): The status code
    ///     * message (&str): What went wrong
    fn respond_error(request: Request, status: u16, message: &str) {
        let _ = request.respond(Response::from_string(message).with_status_code(status));
    }

    /// Sends the current `StateView`, which is what every route that changes
    /// the run state answers with.
    ///
    /// Answering a `POST` with the new state saves the page a second request,
    /// at the cost of one subtlety: a step is only queued here, so the state
    /// returned is the one *before* it runs. The next poll shows the result.
    ///
    /// # Parameters:
    ///     * request (Request): The request to answer
    fn respond_state(&self, request: Request) {
        Self::respond_json(
            request,
            &state_view(
                &self.interpreter,
                self.cycle,
                self.run == RunState::Running,
                self.last_panic.clone(),
            ),
        );
    }

    /// Sends a string as an HTML page.
    ///
    /// # Parameters:
    ///     * request (Request): The request to answer
    ///     * text (&str): The page's markup
    fn respond_html(request: Request, text: &str) {
        let html = Header::from_bytes(&b"Content-Type"[..], &b"text/html"[..])
            .expect("a header written by hand is well formed");
        let _ = request.respond(Response::from_string(text).with_header(html));
    }

    /// Starts the session over on a freshly loaded image.
    ///
    /// Breakpoints deliberately survive. Reloading the same file produces the
    /// same object table, so a location set before the reset still names the
    /// same place, which is what makes setting one and starting over useful.
    ///
    /// On failure the session is left untouched, still running the image it
    /// had: the reload either replaces the interpreter or does nothing.
    ///
    /// # Returns:
    ///     * Result<(), ImageLoadError>, or why the image could not be re-read
    fn reset(&mut self) -> Result<(), ImageLoadError> {
        let image = Image::load(self.image_path.clone())?;
        self.interpreter = Interpreter::from_memory(Image::parse_into_memory(&image)?);
        self.cycle = 0;
        self.run = RunState::Paused;
        self.last_panic = None;
        Ok(())
    }
}

/// Reads an object pointer out of a path like `/object/39508`.
///
/// Decimal because that is what `Described.oop` carries and what the page will
/// put in a link, and because a hex OOP in a URL would have to be escaped or
/// prefixed to be unambiguous.
///
/// # Parameters:
///     * path (&str): The request's path
///     * prefix (&str): The part before the number, e.g. `/object/`
///
/// # Returns:
///     * Option<OOP>, None if the path does not fit or the number does not parse - does not check
///     against SmallInteger
fn oop_from_path(path: &str, prefix: &str) -> Option<OOP> {
    let path = path.split('?').next()?;
    path.strip_prefix(prefix)?
        .parse::<u16>()
        .ok()
        .map(OOP::from_raw)
}

/// Reads one query parameter out of a URL, e.g. `n` from `/step?n=4`.
///
/// # Parameters:
///     * path (&str): The request's path, query string included
///     * name (&str): The parameter to look for
///
/// # Returns:
///     * Option<&str>, the raw value, None if the parameter is absent
fn query_parameter<'a>(path: &'a str, name: &str) -> Option<&'a str> {
    let (_, query) = path.split_once('?')?;

    for pair in query.split('&') {
        if let Some((key, value)) = pair.split_once('=') {
            if key == name {
                return Some(value);
            }
        }
    }

    None
}

/// Extract the message inside the panic box and return it as String
///
/// # Parameters:
///     * payload (&Box<dyn Any + Send>): The panic box
///     
/// # Returns:
///     * String, the message contained in the Box
fn panic_message(payload: &Box<dyn Any + Send>) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|message| message.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic carrying no message".to_string())
}

/// The debugger's page, compiled into the binary.
///
/// Served from `GET /` so that the page and the endpoints it calls share an
/// origin, which is what spares the whole thing a CORS arrangement.
const PAGE: &str = include_str!("web/index.html");

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn snapshot_debugger() -> Debugger {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testBinaries/snapshot.im");
        Debugger::new(path).expect("load failed")
    }

    #[test]
    fn running_to_a_cycle_already_past_stops_at_once() {
        let mut debugger = snapshot_debugger();
        debugger.cycle = 50;

        // A target behind the current cycle would never be reached by an
        // equality test, and the loop would run the image to destruction.
        debugger.run = RunState::UntilCycle(10);
        assert!(debugger.should_pause());

        debugger.run = RunState::UntilCycle(50);
        assert!(
            debugger.should_pause(),
            "the target itself counts as reached"
        );

        debugger.run = RunState::UntilCycle(51);
        assert!(!debugger.should_pause());
    }

    #[test]
    fn stepping_over_runs_until_the_chain_is_back_down() {
        let mut debugger = snapshot_debugger();
        let depth = chain_depth(&debugger.interpreter);

        // Step over: stop once the chain is no deeper than it is now. While a
        // call is in progress it is deeper, and the loop has to keep going.
        debugger.run = RunState::UntilDepth { at_most: depth };
        assert!(debugger.should_pause(), "already at the target depth");

        debugger.run = RunState::UntilDepth { at_most: depth - 1 };
        assert!(!debugger.should_pause(), "step out has not returned yet");

        // A method return can drop the depth by more than one, because a block
        // returns from its home, so the test has to be an inequality.
        debugger.run = RunState::UntilDepth { at_most: depth + 5 };
        assert!(
            debugger.should_pause(),
            "shallower than asked for still stops"
        );
    }

    #[test]
    fn a_breakpoint_stops_a_run_but_not_a_step() {
        let mut debugger = snapshot_debugger();
        let here = (
            debugger.interpreter.method().value,
            debugger.interpreter.ip(),
        );

        debugger.run = RunState::Running;
        assert!(!debugger.should_pause(), "nothing set yet");

        debugger.breakpoints.insert(here);
        assert!(debugger.should_pause());

        // A step the caller asked for lands where it was asked to, even here.
        debugger.run = RunState::Stepping(1);
        assert!(!debugger.should_pause());

        // Every other way of running respects it.
        debugger.run = RunState::UntilCycle(usize::MAX);
        assert!(debugger.should_pause());
    }

    #[test]
    fn running_to_a_location_matches_the_method_and_the_offset() {
        let mut debugger = snapshot_debugger();
        let method = debugger.interpreter.method();
        let ip = debugger.interpreter.ip();

        debugger.run = RunState::UntilLocation { method, ip };
        assert!(debugger.should_pause());

        debugger.run = RunState::UntilLocation { method, ip: ip + 1 };
        assert!(
            !debugger.should_pause(),
            "a different offset is a different place"
        );

        debugger.run = RunState::UntilLocation {
            method: OOP::from_raw(method.value + 2),
            ip,
        };
        assert!(
            !debugger.should_pause(),
            "a different method is a different place"
        );
    }

    #[test]
    fn an_object_path_parses_to_its_pointer() {
        assert_eq!(
            oop_from_path("/object/39508", "/object/"),
            Some(OOP::from_raw(39508))
        );
        // A query string is not part of the number.
        assert_eq!(
            oop_from_path("/method/27492?x=1", "/method/"),
            Some(OOP::from_raw(27492))
        );
        // An odd pointer is a SmallInteger tag rather than an object, which the
        // object view answers for, so it is let through here.
        assert_eq!(
            oop_from_path("/object/1617", "/object/"),
            Some(OOP::from_raw(1617))
        );
    }

    #[test]
    fn a_path_that_is_not_a_pointer_is_no_pointer_at_all() {
        assert_eq!(
            oop_from_path("/object/", "/object/"),
            None,
            "nothing to parse"
        );
        assert_eq!(
            oop_from_path("/object/frog", "/object/"),
            None,
            "not a number"
        );
        assert_eq!(oop_from_path("/chain", "/object/"), None, "wrong route");
        // An OOP is a u16, so a larger number is rejected rather than truncated.
        assert_eq!(oop_from_path("/object/70000", "/object/"), None);
        assert_eq!(oop_from_path("/object/-2", "/object/"), None);
    }

    #[test]
    fn a_query_parameter_is_found_wherever_it_sits() {
        assert_eq!(query_parameter("/step?n=4", "n"), Some("4"));
        assert_eq!(
            query_parameter("/until?method=27492&ip=143", "method"),
            Some("27492")
        );
        assert_eq!(
            query_parameter("/until?method=27492&ip=143", "ip"),
            Some("143")
        );
    }

    #[test]
    fn a_missing_query_parameter_is_none() {
        assert_eq!(query_parameter("/step", "n"), None, "no query string");
        assert_eq!(query_parameter("/step?x=1", "n"), None, "another parameter");
        assert_eq!(
            query_parameter("/step?nn=1", "n"),
            None,
            "not a prefix match"
        );
        assert_eq!(query_parameter("/step?n", "n"), None, "no value");
    }
}
