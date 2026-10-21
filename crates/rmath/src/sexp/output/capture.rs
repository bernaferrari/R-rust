//! output — extracted verbatim from output.rs.

use super::*;

/// UTF8 text captured from R output. Invalid native bytes are represented by
/// ASCII hex escapes; native byte sinks receive the original bytes.
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
    id: u64,
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

#[derive(Clone, Copy)]
struct FrameRoute {
    id: u64,
    older: Option<u64>,
    floor: usize,
    selected: bool,
    connection: Option<(*mut RInstance, i32)>,
    split: bool,
}

impl CaptureFrame {
    /// Return true when this layer consumes the stream completely.
    fn write(&mut self, stream: OutputStream, msg: &[u8], limit: Option<usize>) -> bool {
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
            let previous_len = buffer.len();
            append_bounded_bytes(
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
    next_frame: u64,
}

impl OutputCaptureState {
    pub(crate) fn start(&mut self) {
        self.start_with_options(true, true, false);
    }

    pub(crate) fn start_with_options(&mut self, stdout: bool, stderr: bool, split: bool) {
        let id = self
            .next_frame
            .checked_add(1)
            .unwrap_or_else(|| super::super::context::r_error("capture frame identity exhausted"));
        self.next_frame = id;
        let frame = CaptureFrame {
            id,
            stdout: stdout.then(String::new),
            stderr: stderr.then(String::new),
            interleaved: (stdout || stderr).then(String::new),
            split_stdout: split,
            ..CaptureFrame::default()
        };

        let outer = std::mem::replace(&mut self.current, frame);
        if outer.id != 0 {
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

    fn frame(&self, id: u64) -> Option<&CaptureFrame> {
        if self.current.id == id {
            Some(&self.current)
        } else {
            self.stack.iter().find(|frame| frame.id == id)
        }
    }
    fn frame_mut(&mut self, id: u64) -> Option<&mut CaptureFrame> {
        if self.current.id == id {
            Some(&mut self.current)
        } else {
            self.stack.iter_mut().find(|frame| frame.id == id)
        }
    }
    fn take_frame(&mut self, id: u64) -> Option<RCapturedOutput> {
        if self.current.id == id {
            return Some(self.stop());
        }
        let index = self.stack.iter().position(|frame| frame.id == id)?;
        Some(Self::frame_output(self.stack.remove(index)))
    }
    fn frame_output(frame: CaptureFrame) -> RCapturedOutput {
        RCapturedOutput {
            stdout: frame.stdout.unwrap_or_default(),
            stderr: frame.stderr.unwrap_or_default(),
            interleaved: frame.interleaved.unwrap_or_default(),
            truncated: frame.truncated,
        }
    }
    fn plan(&self, id: u64, stream: OutputStream, depth: usize) -> Option<FrameRoute> {
        let frame = self.frame(id)?;
        let older = if self.current.id == id {
            self.stack.last().map(|f| f.id)
        } else {
            let index = self.stack.iter().position(|f| f.id == id)?;
            index.checked_sub(1).map(|i| self.stack[i].id)
        };
        let selected = match stream {
            OutputStream::Stdout => frame.stdout.is_some(),
            OutputStream::Stderr | OutputStream::Message => frame.stderr.is_some(),
        };
        let floor = if matches!(stream, OutputStream::Stdout) && frame.stdout.is_some() {
            if frame.stderr.is_some() {
                0
            } else {
                frame.sink_depth_at_start.min(depth)
            }
        } else {
            depth
        };
        Some(FrameRoute {
            id,
            older,
            floor,
            selected,
            connection: frame.connection,
            split: matches!(stream, OutputStream::Stdout) && frame.split_stdout,
        })
    }
    // Owned, standalone frame tests use the same callback-free append path.
    // Native destinations require the original runtime route below.
    fn route(&mut self, stream: OutputStream, msg: &[u8]) -> bool {
        for frame in std::iter::once(&mut self.current).chain(self.stack.iter_mut().rev()) {
            if frame.connection.is_some() {
                super::super::context::r_error(
                    "native capture destination requires its original runtime",
                );
            }
            if frame.write(stream, msg, self.max_bytes) {
                return true;
            }
        }
        false
    }
    pub fn capture_stdout(&mut self, msg: &str) -> bool {
        self.route(OutputStream::Stdout, msg.as_bytes())
    }
    pub fn capture_stderr(&mut self, msg: &str) -> bool {
        self.route(OutputStream::Stderr, msg.as_bytes())
    }
    pub(crate) fn set_max_bytes(&mut self, max_bytes: Option<usize>) {
        self.max_bytes = max_bytes;
    }
}

fn append_bounded_bytes(
    target: &mut String,
    message: &[u8],
    limit: Option<usize>,
    used: &mut usize,
    truncated: &mut bool,
) {
    super::native_bytes::visit_capture_text(message, |part| {
        let mut this_truncated = false;
        append_bounded(target, part, limit, used, &mut this_truncated);
        *truncated |= this_truncated;
        !this_truncated
    });
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
    frame: u64,
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
        let frame = unsafe { (*owner.as_ptr()).output_capture.borrow().current.id };
        Self {
            owner,
            active: true,
            frame,
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
        let frame = unsafe { (*instance).output_capture.borrow().current.id };
        Self {
            owner,
            active: true,
            frame,
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
                .frame_mut(self.frame)
                .expect("original capture frame unavailable")
                .connection = Some((self.owner.as_ptr(), index));
        }
    }
    pub(crate) fn finish(mut self) -> RCapturedOutput {
        self.active = false;
        unsafe {
            (*self.owner.as_ptr())
                .output_capture
                .borrow_mut()
                .take_frame(self.frame)
        }
        .unwrap_or_else(|| {
            super::super::context::r_error("original capture frame is no longer available")
        })
    }
}

impl Drop for OutputCaptureGuard {
    fn drop(&mut self) {
        if self.active {
            unsafe {
                let _ = (*self.owner.as_ptr())
                    .output_capture
                    .borrow_mut()
                    .take_frame(self.frame);
            }
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
    capture_stdout_bytes_in(inst, msg.as_bytes());
}
fn call_native<T>(owner: super::super::owner::OwnerToken<'_>, operation: impl FnOnce() -> T) -> T {
    owner
        .require_active()
        .unwrap_or_else(|e| super::super::context::r_error(e.to_string()));
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation));
    owner
        .require_active()
        .unwrap_or_else(|e| super::super::context::r_error(e.to_string()));
    match outcome {
        Ok(value) => value,
        Err(payload) => std::panic::resume_unwind(payload),
    }
}
fn route_runtime(
    owner: super::super::owner::OwnerToken<'_>,
    stream: OutputStream,
    msg: &[u8],
) -> bool {
    let instance = owner.as_ptr();
    let mut id = unsafe { (*instance).output_capture.borrow().current.id };
    let mut depth = unsafe { (*instance).connections_state.sink.sink_number };
    while id != 0 {
        let plan = unsafe { (*instance).output_capture.borrow().plan(id, stream, depth) }
            .unwrap_or_else(|| {
                super::super::context::r_error("original capture frame is no longer available")
            });
        let still_present = || {
            if unsafe { (*instance).output_capture.borrow().frame(plan.id).is_none() } {
                super::super::context::r_error("original capture frame is no longer available");
            }
        };
        if plan.floor < depth {
            let consumed = call_native(owner, || {
                crate::mainutils::connections::write_output_sinks_between(
                    instance, msg, plan.floor, depth,
                )
            });
            still_present();
            depth = plan.floor;
            if consumed {
                return true;
            }
        }
        if plan.selected {
            if let Some((original, index)) = plan.connection {
                if original != instance {
                    super::super::context::r_error(
                        "capture destination belongs to another session",
                    );
                }
                call_native(owner, || {
                    crate::mainutils::connections::connection_write_bytes(index, msg)
                });
                still_present();
                if !plan.split {
                    return true;
                }
            } else {
                let mut capture = unsafe { (*instance).output_capture.borrow_mut() };
                let limit = capture.max_bytes;
                let frame = capture.frame_mut(plan.id).unwrap_or_else(|| {
                    super::super::context::r_error("original capture frame is no longer available")
                });
                if frame.write(stream, msg, limit) {
                    return true;
                }
            }
        }
        id = plan.older.unwrap_or(0);
    }
    false
}
fn capture_stdout_bytes_in(inst: *mut RInstance, msg: &[u8]) {
    let owner = unsafe { super::super::owner::OwnerToken::from_raw(inst) };
    let _pin = owner
        .pin()
        .unwrap_or_else(|e| super::super::context::r_error(e.to_string()));
    if !route_runtime(owner, OutputStream::Stdout, msg)
        && !call_native(owner, || {
            crate::mainutils::connections::write_output_sink_in(inst, msg)
        })
    {
        write_native_stdout(msg);
    }
}

fn write_native_stdout(msg: &[u8]) {
    use std::io::Write;
    if let Err(error) = std::io::stdout().lock().write_all(msg) {
        crate::sexp::context::r_error(format!("error writing native stdout: {error}"));
    }
}
/// Byte-aware native entry. Its caller retains the original runtime and owns
/// any source buffer that a connection callback could otherwise invalidate.
pub(crate) fn emit_native_bytes(owner: Option<super::super::owner::OwnerToken<'_>>, msg: &[u8]) {
    let Some(owner) = owner else {
        write_native_stdout(msg);
        return;
    };
    let _pin = owner
        .pin()
        .unwrap_or_else(|e| super::super::context::r_error(e.to_string()));
    owner
        .require_active()
        .unwrap_or_else(|e| super::super::context::r_error(e.to_string()));
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        capture_stdout_bytes_in(owner.as_ptr(), msg)
    }));
    owner
        .require_active()
        .unwrap_or_else(|e| super::super::context::r_error(e.to_string()));
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
}

/// Append a `message()` to the session's interleaved stream.
///
/// `sink(type="message")` (connection other than the default stderr, 2)
/// receives the text first. Otherwise it joins the capture frame, then
/// real stderr. The session keeps one ordered stream so the text lands
/// between print() side effects, deferred warnings, and auto-printed values.
pub(crate) fn capture_interleaved(msg: &str) {
    super::super::instance::with_current_instance(|inst| {
        capture_error_bytes_in(inst, OutputStream::Message, msg)
    });
}
pub fn capture_stderr(msg: &str) {
    super::super::instance::with_current_instance(|inst| capture_stderr_in(inst, msg));
}
pub fn capture_stderr_in(inst: *mut RInstance, msg: &str) {
    capture_error_bytes_in(inst, OutputStream::Stderr, msg);
}
fn capture_error_bytes_in(inst: *mut RInstance, stream: OutputStream, msg: &str) {
    let owner = unsafe { super::super::owner::OwnerToken::from_raw(inst) };
    let _pin = owner
        .pin()
        .unwrap_or_else(|e| super::super::context::r_error(e.to_string()));
    let error_con = unsafe { (*inst).connections_state.sink.error_con };
    if error_con != 2 {
        call_native(owner, || {
            crate::mainutils::connections::connection_write_bytes(error_con, msg.as_bytes())
        });
        return;
    }
    if !route_runtime(owner, stream, msg.as_bytes()) {
        eprint!("{msg}");
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
/// Valid UTF8 is unchanged; invalid native bytes use the capture text policy.
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

#[cfg(test)]
mod native_byte_tests {
    use super::*;
    #[test]
    fn native_output_capture_prefixes_remain_valid_utf8_at_every_budget() {
        let wire = b"prefix \xc3\xa9\xff\xf0\x9f\x99\x82\xe9";
        let text = "prefix é\\xff🙂\\xe9";
        for limit in 0..=32 {
            let mut state = OutputCaptureState::default();
            state.set_max_bytes(Some(limit));
            state.start();
            assert!(state.route(OutputStream::Stdout, wire));
            let captured = state.stop();
            let mut n = limit.min(text.len());
            while !text.is_char_boundary(n) {
                n -= 1;
            }
            assert_eq!(captured.stdout, &text[..n], "limit {limit}");
            assert_eq!(captured.interleaved, captured.stdout);
            assert_eq!(captured.truncated, limit < text.len());
        }
    }
    #[test]
    fn native_output_nested_split_caps_charge_the_receiving_frame_after_limit_changes() {
        let mut state = OutputCaptureState::default();
        state.set_max_bytes(Some(6));
        state.start();
        state.start_with_options(true, false, true);
        assert!(state.route(OutputStream::Stdout, b"\xc3\xa9\xffX"));
        state.capture_stderr("E");
        let inner = state.stop();
        assert_eq!(inner.stdout, "é\\xff");
        assert!(inner.truncated);
        state.set_max_bytes(Some(9));
        state.route(OutputStream::Stdout, b"Y\xfe");
        let outer = state.stop();
        assert_eq!(outer.stdout, "é\\xffY\\x");
        assert!(outer.stderr.is_empty());
        assert_eq!(outer.interleaved, outer.stdout);
        assert!(outer.truncated);
    }
}

#[cfg(test)]
mod frame_identity_tests {
    use super::*;
    use crate::sexp::RSession;
    #[test]
    fn capture_frame_identity_finish_and_drop_do_not_consume_a_nested_scope() {
        for finish in [true, false] {
            let session = RSession::new_for_gc_tests();
            session.with_active(|| {
                let outer = OutputCaptureGuard::start();
                capture_stdout("outer");
                let inner = OutputCaptureGuard::start();
                capture_stdout("inner");
                if finish {
                    assert_eq!(outer.finish().stdout, "outer");
                } else {
                    drop(outer);
                }
                capture_stdout(" tail");
                assert_eq!(inner.finish().stdout, "inner tail");
                assert!(!is_capturing());
            });
        }
    }
    #[test]
    fn capture_frame_identity_stale_guard_cannot_pop_a_replacement() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let old = OutputCaptureGuard::start();
            capture_stdout("old");
            assert_eq!(stop_capture().stdout, "old");
            let replacement = OutputCaptureGuard::start();
            capture_stdout("replacement");
            let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| old.finish()));
            let error = failed.expect_err("the original frame was explicitly removed");
            assert_eq!(
                error
                    .downcast_ref::<crate::sexp::context::RError>()
                    .unwrap()
                    .message,
                "original capture frame is no longer available"
            );
            capture_stdout(" tail");
            assert_eq!(replacement.finish().stdout, "replacement tail");
            assert!(!is_capturing());
        });
    }
    #[test]
    fn capture_frame_identity_exhaustion_preserves_the_existing_scope() {
        let mut state = OutputCaptureState::default();
        state.start();
        state.capture_stdout("original");
        let id = state.current.id;
        state.next_frame = u64::MAX;
        let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| state.start()));
        assert!(failed.is_err());
        assert_eq!(state.current.id, id);
        assert_eq!(state.stop().stdout, "original");
        assert!(!state.is_capturing());
    }
}
