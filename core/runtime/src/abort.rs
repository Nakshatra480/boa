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

pub(crate) fn abort_error() -> JsError {
    js_error!(Error: "AbortError")
}

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

        for listener in listeners {
            if let Some(object) = listener.as_object()
                && object.is_callable()
            {
                object.call(&JsValue::undefined(), &[], context)?;
            }
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
    #[boa(constructor)]
    pub fn constructor() -> JsResult<Self> {
        Err(js_error!(TypeError: "Illegal constructor"))
    }

    #[boa(getter)]
    pub fn aborted(&self) -> bool {
        self.is_aborted()
    }

    #[boa(getter)]
    pub fn reason(&self) -> JsValue {
        self.reason_value()
    }

    pub fn throw_if_aborted(&self) -> JsResult<()> {
        if self.is_aborted() {
            Err(abort_error())
        } else {
            Ok(())
        }
    }

    pub fn add_event_listener(&self, event_type: JsString, listener: JsValue) -> JsResult<()> {
        if event_type.to_std_string_lossy() != "abort" || !listener.is_callable() {
            return Ok(());
        }

        let mut state = self.state.borrow_mut();
        if state.aborted {
            return Ok(());
        }
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
    #[boa(constructor)]
    pub fn constructor() -> Self {
        Self::new()
    }

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

    pub fn abort(&self, reason: Option<JsValue>, context: &mut Context) -> JsResult<()> {
        let reason = reason.unwrap_or_else(JsValue::undefined);
        self.signal.abort_with_reason(reason, context)
    }
}

#[boa_module]
pub mod js_module {
    type AbortController = super::AbortController;
    type AbortSignal = super::AbortSignal;
}

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
                assert!(after.is_some_and(|value| value.contains("AbortError")));

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
}
