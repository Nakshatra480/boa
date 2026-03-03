use super::TestFetcher;
use crate::fetch::request::JsRequest;
use crate::fetch::response::JsResponse;
use crate::test::{TestAction, run_test_actions};
use boa_engine::{Context, Finalize, JsData, JsResult, JsValue, Trace, js_str, js_string};
use either::Either;
use http::{Response, Uri};
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn request_constructor() {
    run_test_actions([
        TestAction::inspect_context(|ctx| {
            let mut fetcher = TestFetcher::default();
            fetcher.add_response(
                Uri::from_static("http://unit.test"),
                Response::new("Hello World".as_bytes().to_vec()),
            );
            crate::fetch::register(fetcher, None, ctx).expect("failed to register fetch");
        }),
        TestAction::run(
            r#"
                const request = new Request("http://unit.test");
                globalThis.response = fetch(request);
            "#,
        ),
        TestAction::inspect_context(|ctx| {
            let response = ctx.global_object().get(js_str!("response"), ctx).unwrap();
            let response = response.as_promise().unwrap().await_blocking(ctx).unwrap();

            assert_eq!(
                response
                    .as_object()
                    .as_ref()
                    .and_then(|o| o.downcast_ref::<JsResponse>())
                    .unwrap()
                    .body()
                    .as_ref()
                    .as_slice(),
                "Hello World".as_bytes()
            );
        }),
        TestAction::inspect_context(|_ctx| {
            let request =
                JsRequest::create_from_js(Either::Left(js_string!("http://example.com")), None)
                    .unwrap();
            assert_eq!(request.uri().to_string(), "http://example.com/");
        }),
    ]);
}

#[test]
fn request_clone_preserves_body_without_override() {
    run_test_actions([
        TestAction::inspect_context(|ctx| {
            let fetcher = TestFetcher::default();
            crate::fetch::register(fetcher, None, ctx).expect("failed to register fetch");
        }),
        TestAction::run(
            r#"
                const original = new Request("http://unit.test", {
                    method: "POST",
                    body: "payload",
                });
                globalThis.cloned = new Request(original, {
                    headers: { "x-test": "1" },
                });
            "#,
        ),
        TestAction::inspect_context(|ctx| {
            let request = ctx.global_object().get(js_str!("cloned"), ctx).unwrap();
            let request_obj = request.as_object().unwrap();
            let request = request_obj.downcast_ref::<JsRequest>().unwrap();
            assert_eq!(request.inner().body().as_slice(), b"payload");
        }),
    ]);
}

#[test]
fn request_clone_empty_body_preserved() {
    run_test_actions([
        TestAction::inspect_context(|ctx| {
            let fetcher = TestFetcher::default();
            crate::fetch::register(fetcher, None, ctx).expect("failed to register fetch");
        }),
        TestAction::run(
            r#"
                const original = new Request("http://unit.test", {
                    method: "POST",
                    body: "",
                });
                globalThis.cloned = new Request(original, {
                    headers: { "x-test": "1" },
                });
            "#,
        ),
        TestAction::inspect_context(|ctx| {
            let request = ctx.global_object().get(js_str!("cloned"), ctx).unwrap();
            let request_obj = request.as_object().unwrap();
            let request = request_obj.downcast_ref::<JsRequest>().unwrap();
            assert_eq!(request.inner().body().as_slice(), b"");
        }),
    ]);
}

#[test]
fn request_clone_body_override() {
    run_test_actions([
        TestAction::inspect_context(|ctx| {
            let fetcher = TestFetcher::default();
            crate::fetch::register(fetcher, None, ctx).expect("failed to register fetch");
        }),
        TestAction::run(
            r#"
                const original = new Request("http://unit.test", {
                    method: "POST",
                    body: "payload",
                });
                globalThis.cloned = new Request(original, {
                    body: "override",
                });
            "#,
        ),
        TestAction::inspect_context(|ctx| {
            let request = ctx.global_object().get(js_str!("cloned"), ctx).unwrap();
            let request_obj = request.as_object().unwrap();
            let request = request_obj.downcast_ref::<JsRequest>().unwrap();
            assert_eq!(request.inner().body().as_slice(), b"override");
        }),
    ]);
}

#[test]
fn request_clone_no_body_preserved() {
    run_test_actions([
        TestAction::inspect_context(|ctx| {
            let fetcher = TestFetcher::default();
            crate::fetch::register(fetcher, None, ctx).expect("failed to register fetch");
        }),
        TestAction::run(
            r#"
                const original = new Request("http://unit.test");
                globalThis.cloned = new Request(original, {
                    headers: { "x-test": "1" },
                });
            "#,
        ),
        TestAction::inspect_context(|ctx| {
            let request = ctx.global_object().get(js_str!("cloned"), ctx).unwrap();
            let request_obj = request.as_object().unwrap();
            let request = request_obj.downcast_ref::<JsRequest>().unwrap();
            assert_eq!(request.inner().body().as_slice(), b"");
        }),
    ]);
}

#[derive(Debug, Clone, Trace, Finalize, JsData)]
struct AbortOnFetchFetcher;

impl crate::fetch::Fetcher for AbortOnFetchFetcher {
    async fn fetch(
        self: Rc<Self>,
        request: JsRequest,
        context: &RefCell<&mut Context>,
    ) -> JsResult<JsResponse> {
        if let Some(signal) = request.signal_value() {
            signal.abort_with_reason(JsValue::undefined(), &mut context.borrow_mut())?;
        }
        Ok(JsResponse::basic(
            "http://unit.test".into(),
            Response::new(Vec::new()),
        ))
    }
}

#[test]
fn fetch_rejects_when_signal_preaborted() {
    run_test_actions([
        TestAction::inspect_context(|ctx| {
            crate::fetch::register(TestFetcher::default(), None, ctx)
                .expect("failed to register fetch");
        }),
        TestAction::run(
            r#"
                const controller = new AbortController();
                controller.abort();
                globalThis.promise = fetch("http://unit.test", { signal: controller.signal });
            "#,
        ),
        TestAction::inspect_context(|ctx| {
            let promise = ctx.global_object().get(js_str!("promise"), ctx).unwrap();
            let err = promise
                .as_promise()
                .unwrap()
                .await_blocking(ctx)
                .unwrap_err();
            assert!(err.to_string().contains("The operation was aborted"));
        }),
    ]);
}

#[test]
fn fetch_rejects_when_signal_aborted_during_fetch() {
    run_test_actions([
        TestAction::inspect_context(|ctx| {
            crate::fetch::register(AbortOnFetchFetcher, None, ctx)
                .expect("failed to register fetch");
        }),
        TestAction::run(
            r#"
                const controller = new AbortController();
                globalThis.promise = fetch("http://unit.test", { signal: controller.signal });
            "#,
        ),
        TestAction::inspect_context(|ctx| {
            let promise = ctx.global_object().get(js_str!("promise"), ctx).unwrap();
            let err = promise
                .as_promise()
                .unwrap()
                .await_blocking(ctx)
                .unwrap_err();
            assert!(err.to_string().contains("The operation was aborted"));
        }),
    ]);
}

#[test]
fn fetch_rejects_when_signal_reused() {
    run_test_actions([
        TestAction::inspect_context(|ctx| {
            crate::fetch::register(TestFetcher::default(), None, ctx)
                .expect("failed to register fetch");
        }),
        TestAction::run(
            r#"
                const controller = new AbortController();
                controller.abort();
                globalThis.p1 = fetch("http://unit.test", { signal: controller.signal });
                globalThis.p2 = fetch("http://unit.test", { signal: controller.signal });
            "#,
        ),
        TestAction::inspect_context(|ctx| {
            let p1 = ctx.global_object().get(js_str!("p1"), ctx).unwrap();
            let err = p1.as_promise().unwrap().await_blocking(ctx).unwrap_err();
            assert!(err.to_string().contains("The operation was aborted"));

            let p2 = ctx.global_object().get(js_str!("p2"), ctx).unwrap();
            let err = p2.as_promise().unwrap().await_blocking(ctx).unwrap_err();
            assert!(err.to_string().contains("The operation was aborted"));
        }),
    ]);
}
