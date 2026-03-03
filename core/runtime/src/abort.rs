//! Implementation of the `AbortController` and `AbortSignal` Web APIs.
//!
//! More information:
//!  - [MDN `AbortController`][mdn-ctrl]
//!  - [MDN `AbortSignal`][mdn-sig]
//!  - [DOM specification][spec]
//!
//! [mdn-ctrl]: https://developer.mozilla.org/en-US/docs/Web/API/AbortController
//! [mdn-sig]: https://developer.mozilla.org/en-US/docs/Web/API/AbortSignal
//! [spec]: https://dom.spec.whatwg.org/#interface-abortcontroller

use boa_engine::class::Class;
use boa_engine::realm::Realm;
use boa_engine::value::TryFromJs;
use boa_engine::{
    Context, Finalize, JsData, JsError, JsObject, JsResult, JsString, JsValue, Trace, boa_class,
    boa_module, js_error,
};
use boa_gc::{Finalize as GcFinalize, Gc, GcRefCell, Trace as GcTrace};

#[derive(Debug, GcTrace, GcFinalize)]
struct AbortSignalState {
    aborted: bool,
    reason: JsValue,
    listeners: Vec<JsValue>,
}

impl AbortSignalState {
    fn new() -> Self {
        Self {
            aborted: false,
            reason: JsValue::undefined(),
            listeners: Vec::new(),
        }
    }
}

/// Create a `JsError` representing an abort error, matching the behaviour
/// of a `DOMException` with name `"AbortError"`.
pub(crate) fn abort_error() -> JsError {
    js_error!(Error: "The operation was aborted")
}

/// The `AbortSignal` class, representing a signal object that communicates
/// with an `AbortController` and can be used to abort an operation.
#[derive(Clone, Debug, Trace, Finalize, JsData)]
pub struct AbortSignal {
    state: Gc<GcRefCell<AbortSignalState>>,
}

impl AbortSignal {
    pub(crate) fn new() -> Self {
        Self {
            state: Gc::new(GcRefCell::new(AbortSignalState::new())),
        }
    }

    pub(crate) fn is_aborted(&self) -> bool {
        self.state.borrow().aborted
    }

    pub(crate) fn reason_value(&self) -> JsValue {
        self.state.borrow().reason.clone()
    }

    pub(crate) fn abort_with_reason(&self, reason: JsValue, context: &mut Context) -> JsResult<()> {
        let listeners = {
            let mut state = self.state.borrow_mut();
            if state.aborted {
                return Ok(());
            }
            state.aborted = true;
            state.reason = reason;
            std::mem::take(&mut state.listeners)
        };

        let mut first_error = None;
        for listener in listeners {
            if let Some(object) = listener.as_object()
                && object.is_callable()
                && let Err(e) = object.call(&JsValue::undefined(), &[], context)
            {
                first_error.get_or_insert(e);
            }
        }

        if let Some(e) = first_error {
            return Err(e);
        }
        Ok(())
    }
}

impl TryFromJs for AbortSignal {
    fn try_from_js(value: &JsValue, _context: &mut Context) -> JsResult<Self> {
        let Some(object) = value.as_object() else {
            return Err(js_error!(TypeError: "AbortSignal must be an object"));
        };
        let Ok(signal) = object.clone().downcast::<AbortSignal>() else {
            return Err(js_error!(TypeError: "AbortSignal expected"));
        };
        Ok(signal.borrow().data().clone())
    }
}

#[boa_class(rename = "AbortSignal")]
#[boa(rename_all = "camelCase")]
impl AbortSignal {
    /// `AbortSignal` cannot be constructed directly.
    ///
    /// # Errors
    /// Always returns a `TypeError`.
    #[boa(constructor)]
    pub fn constructor() -> JsResult<Self> {
        Err(js_error!(TypeError: "Illegal constructor"))
    }

    /// Returns `true` if the signal has been aborted.
    #[must_use]
    #[boa(getter)]
    pub fn aborted(&self) -> bool {
        self.is_aborted()
    }

    /// Returns the abort reason, or `undefined` if not aborted.
    #[must_use]
    #[boa(getter)]
    pub fn reason(&self) -> JsValue {
        self.reason_value()
    }

    /// Throws the abort reason if the signal has been aborted.
    ///
    /// # Errors
    /// Returns the stored abort reason as an error if the signal is aborted.
    pub fn throw_if_aborted(&self) -> JsResult<()> {
        if self.is_aborted() {
            // Per spec, throwIfAborted re-throws the stored reason.
            Err(JsError::from_opaque(self.reason_value()))
        } else {
            Ok(())
        }
    }

    /// Register a listener for the `"abort"` event.
    ///
    /// # Errors
    /// Returns an error if the listener invocation fails on an already-aborted signal.
    pub fn add_event_listener(
        &self,
        event_type: JsString,
        listener: JsValue,
        context: &mut Context,
    ) -> JsResult<()> {
        if event_type.to_std_string_lossy() != "abort" || !listener.is_callable() {
            return Ok(());
        }

        // If already aborted, fire the listener immediately per spec.
        let already_aborted = self.state.borrow().aborted;
        if already_aborted {
            if let Some(object) = listener.as_object()
                && object.is_callable()
            {
                object.call(&JsValue::undefined(), &[], context)?;
            }
            return Ok(());
        }

        let mut state = self.state.borrow_mut();
        if state
            .listeners
            .iter()
            .any(|existing| JsValue::same_value(existing, &listener))
        {
            return Ok(());
        }
        state.listeners.push(listener);
        Ok(())
    }

    /// Remove a previously registered `"abort"` event listener.
    ///
    /// # Errors
    /// This method does not currently return errors but returns `JsResult`
    /// for API consistency.
    pub fn remove_event_listener(&self, event_type: JsString, listener: JsValue) -> JsResult<()> {
        if event_type.to_std_string_lossy() != "abort" {
            return Ok(());
        }
        let mut state = self.state.borrow_mut();
        state
            .listeners
            .retain(|existing| !JsValue::same_value(existing, &listener));
        Ok(())
    }
}

/// The `AbortController` class, used to cancel ongoing operations such
/// as fetch requests.
#[derive(Clone, Debug, Trace, Finalize, JsData)]
pub struct AbortController {
    signal: AbortSignal,
    signal_object: Gc<GcRefCell<Option<JsObject>>>,
}

impl AbortController {
    fn new() -> Self {
        Self {
            signal: AbortSignal::new(),
            signal_object: Gc::new(GcRefCell::new(None)),
        }
    }
}

#[boa_class(rename = "AbortController")]
#[boa(rename_all = "camelCase")]
impl AbortController {
    /// Create a new `AbortController`.
    #[must_use]
    #[boa(constructor)]
    pub fn constructor() -> Self {
        Self::new()
    }

    /// Get the `AbortSignal` associated with this controller.
    ///
    /// # Errors
    /// Returns an error if the signal object cannot be created.
    #[boa(getter)]
    pub fn signal(&self, context: &mut Context) -> JsResult<JsValue> {
        let mut cached = self.signal_object.borrow_mut();
        if let Some(signal) = cached.as_ref() {
            return Ok(signal.clone().into());
        }
        let signal = AbortSignal::from_data(self.signal.clone(), context)?;
        *cached = Some(signal.clone());
        Ok(signal.into())
    }

    /// Signal abort, optionally providing a custom reason.
    ///
    /// # Errors
    /// Returns an error if notifying listeners fails.
    pub fn abort(&self, reason: Option<JsValue>, context: &mut Context) -> JsResult<()> {
        // Per spec, the default reason is a new AbortError DOMException.
        let reason = match reason {
            Some(r) => r,
            None => abort_error().into_opaque(context)?,
        };
        self.signal.abort_with_reason(reason, context)
    }
}

/// JavaScript module exposing `AbortController` and `AbortSignal`.
#[boa_module]
pub mod js_module {
    type AbortController = super::AbortController;
    type AbortSignal = super::AbortSignal;
}

/// Register the `AbortController` and `AbortSignal` classes in the given realm.
///
/// # Errors
/// Returns an error if class registration fails.
pub fn register(realm: Option<Realm>, context: &mut Context) -> JsResult<()> {
    js_module::boa_register(realm, context)
}

#[cfg(test)]
mod tests {
    use crate::test::{TestAction, run_test_actions};
    use boa_engine::{js_str, js_string};

    #[test]
    fn abort_signal_listeners_and_idempotent() {
        run_test_actions([
            TestAction::run(
                r#"
                    const controller = new AbortController();
                    let calls = 0;
                    const a = () => { calls += 1; };
                    const b = () => { calls += 10; };
                    controller.signal.addEventListener("abort", a);
                    controller.signal.addEventListener("abort", b);
                    controller.abort();
                    controller.abort();
                    globalThis.calls = calls;
                "#,
            ),
            TestAction::inspect_context(|ctx| {
                let calls = ctx.global_object().get(js_str!("calls"), ctx).unwrap();
                let calls = calls.as_number().unwrap();
                assert!((calls - 11.0).abs() < f64::EPSILON);
            }),
        ]);
    }

    #[test]
    fn abort_signal_throw_if_aborted_and_reason() {
        run_test_actions([
            TestAction::run(
                r#"
                    const controller = new AbortController();
                    let before = "unset";
                    try {
                        controller.signal.throwIfAborted();
                        before = "ok";
                    } catch (e) {
                        before = "threw";
                    }
                    const s1 = controller.signal;
                    const s2 = controller.signal;
                    globalThis.signalStable = s1 === s2;
                    controller.abort("custom-reason");
                    let after;
                    try {
                        controller.signal.throwIfAborted();
                        after = "no-throw";
                    } catch (e) {
                        after = e.toString();
                    }
                    globalThis.before = before;
                    globalThis.after = after;
                    globalThis.reason = controller.signal.reason;
                "#,
            ),
            TestAction::inspect_context(|ctx| {
                let before = ctx.global_object().get(js_str!("before"), ctx).unwrap();
                assert_eq!(before.as_string(), Some(js_string!("ok")));

                let signal_stable = ctx
                    .global_object()
                    .get(js_str!("signalStable"), ctx)
                    .unwrap();
                assert_eq!(signal_stable.as_boolean(), Some(true));

                let after = ctx.global_object().get(js_str!("after"), ctx).unwrap();
                let after = after.as_string().map(|value| value.to_std_string_escaped());
                assert!(after.is_some_and(|value| value.contains("custom-reason")));

                let reason = ctx.global_object().get(js_str!("reason"), ctx).unwrap();
                assert_eq!(reason.as_string(), Some(js_string!("custom-reason")));
            }),
        ]);
    }

    #[test]
    fn abort_signal_remove_event_listener() {
        run_test_actions([
            TestAction::run(
                r#"
                    const controller = new AbortController();
                    let calls = 0;
                    const listener = () => { calls += 1; };
                    controller.signal.addEventListener("abort", listener);
                    controller.signal.removeEventListener("abort", listener);
                    controller.abort();
                    globalThis.calls = calls;
                "#,
            ),
            TestAction::inspect_context(|ctx| {
                let calls = ctx.global_object().get(js_str!("calls"), ctx).unwrap();
                let calls = calls.as_number().unwrap();
                assert!((calls - 0.0).abs() < f64::EPSILON);
            }),
        ]);
    }

    #[test]
    fn abort_signal_listener_fires_on_already_aborted() {
        run_test_actions([
            TestAction::run(
                r#"
                    const controller = new AbortController();
                    controller.abort();
                    let fired = false;
                    controller.signal.addEventListener("abort", () => { fired = true; });
                    globalThis.fired = fired;
                "#,
            ),
            TestAction::inspect_context(|ctx| {
                let fired = ctx.global_object().get(js_str!("fired"), ctx).unwrap();
                assert_eq!(fired.as_boolean(), Some(true));
            }),
        ]);
    }
}
