//! output — extracted verbatim from output.rs.

use super::*;

/// Captured R output.
#[derive(Debug, Clone, Default)]
pub struct RCapturedOutput {
    pub stdout: String,
    pub stderr: String,
    /// Chronological stdout+stderr for embed hosts (GNU terminal order).
    pub interleaved: String,
    pub truncated: bool,
}

/// One capture layer owns its budget, including traffic forwarded to it.
#[derive(Debug, Default)]
pub(crate) struct CaptureFrame {
    pub(crate) stdout: Option<String>,
    pub(crate) stderr: Option<String>,
    pub(crate) interleaved: Option<String>,
    pub(crate) truncated: bool,
    pub(crate) used_bytes: usize,
    split_stdout: bool,
    sink_depth_at_start: usize,
    connection: Option<(*mut RInstance, i32)>,
}

#[derive(Clone, Copy)]
pub(crate) enum OutputStream {
    Stdout,
    Stderr,
    Message,
}

impl CaptureFrame {
    /// Return true when this layer consumes the stream completely.
    fn write(&mut self, stream: OutputStream, msg: &str, limit: Option<usize>) -> bool {
        let (target, split) = match stream {
            OutputStream::Stdout => (&mut self.stdout, self.split_stdout),
            OutputStream::Stderr => (&mut self.stderr, false),
            // GNU message() writes stderr. An explicit message capture
            // (capture.output(type="message")) owns stderr; an output-only
            // capture must let messages continue outward as stderr.
            OutputStream::Message if self.stderr.is_some() => (&mut self.stderr, false),
            OutputStream::Message => return false,
        };
        if let Some(buffer) = target {
            if let Some((owner, connection)) = self.connection {
                if super::super::instance::current_instance_ptr() != Some(owner) {
                    std::panic::panic_any(super::super::context::RError {
                        message: "capture destination belongs to another session".into(),
                    });
                }
                crate::mainutils::connections::connection_write_bytes(connection, msg.as_bytes());
                return !split;
            }
            let previous_len = buffer.len();
            append_bounded(
                buffer,
                msg,
                limit,
                &mut self.used_bytes,
                &mut self.truncated,
            );
            if let Some(interleaved) = &mut self.interleaved {
                // Mirror exactly the admitted prefix, including UTF-8 truncation.
                // Copying the original message here would bypass this frame's cap.
                interleaved.push_str(&buffer[previous_len..]);
            }
            return !split;
        }
        false
    }
    fn active(&self) -> bool {
        self.stdout.is_some() || self.stderr.is_some()
    }
}

/// Per-session output capture buffers.
#[derive(Debug, Default)]
pub(crate) struct OutputCaptureState {
    pub(crate) current: CaptureFrame,
    stack: Vec<CaptureFrame>,
    max_bytes: Option<usize>,
}

impl OutputCaptureState {
    pub(crate) fn start(&mut self) {
        self.start_with_options(true, true, false);
    }

    pub(crate) fn start_with_options(&mut self, stdout: bool, stderr: bool, split: bool) {
        let frame = CaptureFrame {
            stdout: stdout.then(String::new),
            stderr: stderr.then(String::new),
            interleaved: (stdout || stderr).then(String::new),
            split_stdout: split,
            ..CaptureFrame::default()
        };

        let outer = std::mem::replace(&mut self.current, frame);
        if outer.active() {
            self.stack.push(outer);
        }
    }

    pub(crate) fn uses_connection(&self, index: i32) -> bool {
        self.current
            .connection
            .is_some_and(|(_, connection)| connection == index)
            || self.stack.iter().any(|frame| {
                frame
                    .connection
                    .is_some_and(|(_, connection)| connection == index)
            })
    }

    pub(crate) fn stop(&mut self) -> RCapturedOutput {
        let frame = std::mem::replace(&mut self.current, self.stack.pop().unwrap_or_default());
        RCapturedOutput {
            stdout: frame.stdout.unwrap_or_default(),
            stderr: frame.stderr.unwrap_or_default(),
            interleaved: frame.interleaved.unwrap_or_default(),
            truncated: frame.truncated,
        }
    }

    pub fn is_capturing(&self) -> bool {
        self.current.active()
    }

    fn route(&mut self, stream: OutputStream, msg: &str) -> bool {
        self.route_with_sink(stream, msg, None)
    }
    fn route_with_sink(
        &mut self,
        stream: OutputStream,
        msg: &str,
        instance: Option<*mut RInstance>,
    ) -> bool {
        let mut pending_depth = instance.map_or(0, |instance| unsafe {
            (*instance).connections_state.sink.sink_number
        });
        for frame in std::iter::once(&mut self.current).chain(self.stack.iter_mut().rev()) {
            if matches!(stream, OutputStream::Stdout) && frame.stdout.is_some() {
                let floor = if frame.stderr.is_some() {
                    0
                } else {
                    frame.sink_depth_at_start.min(pending_depth)
                };
                if let Some(instance) = instance {
                    if crate::mainutils::connections::write_output_sinks_between(
                        instance,
                        msg.as_bytes(),
                        floor,
                        pending_depth,
                    ) {
                        return true;
                    }
                }
                pending_depth = floor;
            }
            if frame.write(stream, msg, self.max_bytes) {
                return true;
            }
        }
        false
    }
    pub fn capture_stdout(&mut self, msg: &str) -> bool {
        self.route(OutputStream::Stdout, msg)
    }
    pub fn capture_stderr(&mut self, msg: &str) -> bool {
        self.route(OutputStream::Stderr, msg)
    }
    pub(crate) fn set_max_bytes(&mut self, max_bytes: Option<usize>) {
        self.max_bytes = max_bytes;
    }
}

pub(crate) fn append_bounded(
    target: &mut String,
    message: &str,
    max_bytes: Option<usize>,
    used_bytes: &mut usize,
    truncated: &mut bool,
) {
    let Some(limit) = max_bytes else {
        target.push_str(message);
        return;
    };
    if message.is_empty() {
        return;
    }
    if *used_bytes >= limit {
        *truncated = true;
        return;
    }
    let remaining = limit - *used_bytes;
    if message.len() <= remaining {
        target.push_str(message);
        *used_bytes += message.len();
        return;
    }
    let mut end = remaining;
    while end > 0 && !message.is_char_boundary(end) {
        end -= 1;
    }
    target.push_str(&message[..end]);
    *used_bytes += end;
    *truncated = true;
}

/// Start capturing R output.
pub fn start_capture() {
    super::super::instance::with_required_current_instance(start_capture_in);
}

pub(crate) fn start_capture_in(inst: *mut RInstance) {
    // P2: strictly-local RefCell write; no ambient write intervenes.
    unsafe {
        (*inst).output_capture.borrow_mut().start();
    }
}

/// Stop capturing and return the captured output.
pub fn stop_capture() -> RCapturedOutput {
    super::super::instance::with_required_current_instance(stop_capture_in)
}

pub(crate) fn stop_capture_in(inst: *mut RInstance) -> RCapturedOutput {
    // P2: strictly-local RefCell read/write; no ambient write intervenes.
    unsafe { (*inst).output_capture.borrow_mut().stop() }
}

/// A nested capture owned by its starting session, restored even on errors.
/// Discarding an unfinished capture mirrors capture.output(file=NULL) on error.
pub(crate) struct OutputCaptureGuard {
    owner: super::super::owner::OwnerPin,
    active: bool,
}

impl OutputCaptureGuard {
    fn owner() -> super::super::owner::OwnerPin {
        // The current token is used only to authenticate its original managed
        // owner; the returned pin, rather than the token, retains storage.
        unsafe { super::super::owner::OwnerToken::current() }
            .expect("output capture requires a current runtime")
            .weak_owner()
            .expect("output capture requires a managed runtime")
            .pin()
            .expect("output capture requires an active runtime")
    }

    pub(crate) fn start() -> Self {
        let owner = Self::owner();
        start_capture_in(owner.as_ptr());
        Self {
            owner,
            active: true,
        }
    }
    pub(crate) fn start_with_options(stdout: bool, stderr: bool, split: bool) -> Self {
        let owner = Self::owner();
        let instance = owner.as_ptr();
        unsafe {
            let depth = (*instance).connections_state.sink.sink_number;
            let mut capture = (*instance).output_capture.borrow_mut();
            capture.start_with_options(stdout, stderr, split);
            capture.current.sink_depth_at_start = depth;
        }
        Self {
            owner,
            active: true,
        }
    }
    pub(crate) fn set_connection(&mut self, index: i32) {
        self.owner
            .require_live()
            .expect("output capture runtime was closed");
        unsafe {
            (*self.owner.as_ptr())
                .output_capture
                .borrow_mut()
                .current
                .connection = Some((self.owner.as_ptr(), index));
        }
    }
    pub(crate) fn finish(mut self) -> RCapturedOutput {
        let output = stop_capture_in(self.owner.as_ptr());
        self.active = false;
        output
    }
}

impl Drop for OutputCaptureGuard {
    fn drop(&mut self) {
        if self.active {
            let _ = stop_capture_in(self.owner.as_ptr());
        }
    }
}

/// Check if output capture is active.
pub fn is_capturing() -> bool {
    super::super::instance::with_current_instance(is_capturing_in).unwrap_or(false)
}

pub fn is_capturing_in(inst: *mut RInstance) -> bool {
    // P2: strictly-local RefCell read; no ambient write intervenes.
    let capture_active = unsafe { (*inst).output_capture.borrow().is_capturing() };
    capture_active || crate::mainutils::connections::output_sink_active_in(inst)
}

/// Append to captured stdout. Called by the Rprintf hook.
pub fn capture_stdout(msg: &str) {
    super::super::instance::with_current_instance(|inst| capture_stdout_in(inst, msg));
}

pub fn capture_stdout_in(inst: *mut RInstance, msg: &str) {
    // P2: the RefCell borrow below is dropped before the print!, and no
    // ambient write occurs while it is held.
    let mut capture = unsafe { (*inst).output_capture.borrow_mut() };
    if capture.route_with_sink(OutputStream::Stdout, msg, Some(inst)) {
        return;
    }
    drop(capture);
    if !crate::mainutils::connections::write_output_sink_in(inst, msg.as_bytes()) {
        print!("{msg}");
    }
}

/// Append a `message()` to the session's interleaved stream.
///
/// `sink(type="message")` (connection other than the default stderr, 2)
/// receives the text first. Otherwise it joins the capture frame, then
/// real stderr. The session keeps one ordered stream so the text lands
/// between print() side effects, deferred warnings, and auto-printed values.
pub(crate) fn capture_interleaved(msg: &str) {
    super::super::instance::with_current_instance(|inst| unsafe {
        // GNU `sink(type="message")` diverts message() before any capture
        // frame. Connection 2 is the default stderr and stays on the frame.
        let error_con = crate::mainutils::connections::sink_state().error_con;
        if error_con != 2 {
            crate::mainutils::connections::connection_write_bytes(error_con, msg.as_bytes());
            return;
        }
        let captured = (*inst)
            .output_capture
            .borrow_mut()
            .route(OutputStream::Message, msg);
        if !captured {
            eprint!("{msg}");
        }
    });
}

/// Append to captured stderr. Called by the REprintf hook.
pub fn capture_stderr(msg: &str) {
    super::super::instance::with_current_instance(|inst| capture_stderr_in(inst, msg));
}

pub fn capture_stderr_in(inst: *mut RInstance, msg: &str) {
    unsafe {
        let error_con = crate::mainutils::connections::sink_state().error_con;
        if error_con != 2 {
            crate::mainutils::connections::connection_write_bytes(error_con, msg.as_bytes());
            return;
        }
        let mut capture = (*inst).output_capture.borrow_mut();
        if !capture.capture_stderr(msg) {
            drop(capture);
            eprint!("{msg}");
        }
    }
}

thread_local! {
    static SHOW_STDOUT_BEFORE_ERROR: std::cell::RefCell<String> =
        const { std::cell::RefCell::new(String::new()) };
}

/// `show()` stdout captured before `stop()`, kept across the panic.
pub(crate) fn take_show_stdout_before_error() -> String {
    SHOW_STDOUT_BEFORE_ERROR.with(|slot| std::mem::take(&mut *slot.borrow_mut()))
}

pub(crate) fn keep_show_stdout_before_error(text: &str) {
    if text.is_empty() {
        return;
    }
    // This frame printed before a nested show that already saved its buffer.
    SHOW_STDOUT_BEFORE_ERROR.with(|slot| {
        let mut saved = slot.borrow_mut();
        if saved.is_empty() {
            saved.push_str(text);
            return;
        }
        let mut combined = String::with_capacity(text.len() + saved.len());
        combined.push_str(text);
        combined.push_str(&saved);
        *saved = combined;
    });
}

#[derive(Debug)]
pub(crate) struct PrintedOutputError {
    pub(crate) message: String,
    pub(crate) captured: RCapturedOutput,
}

/// Capture actual automatic printing, including custom methods and both streams.
/// Presentation formatting never modifies these emitted bytes.
pub(crate) fn capture_printed_value(x: Sexp<'_>) -> Result<RCapturedOutput, PrintedOutputError> {
    // Only the managed original authority escapes this temporary current token.
    let original = unsafe { super::super::owner::OwnerToken::current() }
        .expect("automatic printing requires a current runtime")
        .weak_owner()
        .expect("automatic printing requires a managed runtime");
    let guard = OutputCaptureGuard::start();
    let printed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        super::super::owner::with_runtime(&original, |access| {
            access.domain().link(&x)?;
            print_value(x);
            Ok::<(), super::super::object::SexpError>(())
        })
    }));
    let captured = guard.finish();
    match printed {
        Ok(Ok(Ok(()))) => Ok(captured),
        Ok(Err(error)) | Ok(Ok(Err(error))) => Err(PrintedOutputError {
            message: error.to_string(),
            captured,
        }),
        Err(payload) => {
            let message = if !original.is_live() {
                Some(super::super::object::SexpError::RootUnavailable.to_string())
            } else if let Some(error) = payload.downcast_ref::<super::super::context::RError>() {
                Some(error.message.clone())
            } else if let Some(super::super::context::RSignal::Error { message }) =
                payload.downcast_ref::<super::super::context::RSignal>()
            {
                Some(message.clone())
            } else {
                None
            };
            match message {
                Some(message) => Err(PrintedOutputError { message, captured }),
                None => std::panic::resume_unwind(payload),
            }
        }
    }
}

pub(crate) fn emit(msg: &str) {
    if is_capturing() {
        capture_stdout(msg);
    } else {
        print!("{}", msg);
    }
}

pub(crate) fn result_admission_error(x: Sexp<'_>) -> Option<String> {
    let limit = super::super::instance::with_current_instance(|inst| {
        // SAFETY: current instance is active; this short immutable borrow ends
        // before traversing the rooted result, and performs no R allocation.
        unsafe { (*inst).output_capture.borrow().max_bytes }
    })
    .flatten()?;
    crate::android::result_budget::admit(x, limit)
        .err()
        .map(|message| format!("[{message}]"))
}
