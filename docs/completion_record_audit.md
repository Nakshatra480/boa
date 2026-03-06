# CompletionRecord Audit — vm/opcode/ and builtins/generator/

Audit of every site that **emits** or **matches on** a `CompletionRecord` variant
within the two directories scoped by issue #2675 plus the central VM handler
functions in `vm/mod.rs` that they delegate to.

## Emit sites

| File                               | Function / impl                             | Variant  | Spec-correct? | Notes                                                         |
| ---------------------------------- | ------------------------------------------- | -------- | ------------- | ------------------------------------------------------------- |
| `vm/mod.rs`                        | `handle_return`                             | `Return` | ✅ Yes        | Fixed in this PR (was `Normal`)                               |
| `vm/mod.rs`                        | `handle_yield`                              | `Normal` | ✅ Yes        | Fixed in this PR (was `Return`)                               |
| `vm/mod.rs`                        | `handle_throw` (2 sites)                    | `Throw`  | ✅ Yes        | Correctly propagates pending exception                        |
| `vm/mod.rs`                        | `handle_error` (non-catchable path)         | `Throw`  | ✅ Yes        | Bubbles uncatchable errors to caller                          |
| `vm/mod.rs`                        | `run` / `run_async_with_budget` fallthrough | `Throw`  | ✅ Yes        | End-of-bytecode error                                         |
| `vm/opcode/control_flow/return.rs` | `Return::operation`                         | —        | ✅ Yes        | Delegates to `handle_return`                                  |
| `vm/opcode/control_flow/throw.rs`  | `Throw::operation`                          | —        | ✅ Yes        | Sets pending exception, delegates to `handle_throw`           |
| `vm/opcode/control_flow/throw.rs`  | `ReThrow::operation`                        | —        | ✅ Yes        | Delegates to `handle_return` (no exception) or `handle_throw` |
| `vm/opcode/generator/mod.rs`       | `Generator::operation`                      | —        | ✅ Yes        | Delegates to `handle_yield`                                   |
| `vm/opcode/generator/mod.rs`       | `AsyncGenerator::operation`                 | —        | ✅ Yes        | Delegates to `handle_yield`                                   |
| `vm/opcode/generator/yield_stm.rs` | `GeneratorYield::operation`                 | —        | ✅ Yes        | Delegates to `handle_yield`                                   |
| `vm/opcode/generator/yield_stm.rs` | `AsyncGeneratorYield::operation`            | —        | ✅ Yes        | Delegates to `handle_yield` (for suspension path)             |
| `vm/opcode/await/mod.rs`           | `Await::operation`                          | —        | ✅ Yes        | Delegates to `handle_yield`                                   |

## Match / consume sites

| File                               | Function / impl                        | Matches on                                                           | Spec-correct? | Notes                                                                             |
| ---------------------------------- | -------------------------------------- | -------------------------------------------------------------------- | ------------- | --------------------------------------------------------------------------------- |
| `builtins/generator/mod.rs`        | `handle_resumption_result`             | `Normal`→SuspendedYield, `Return`→Completed, `Throw`→Completed       | ✅ Yes        | Refactored in this PR; shared by `generator_resume` and `generator_resume_abrupt` |
| `vm/opcode/generator/yield_stm.rs` | `AsyncGeneratorYield` (queue dispatch) | `Normal`→Normal resume, `Return`→Return resume, `Throw`→Throw resume | ✅ Yes        | Maps variants to `GeneratorResumeKind`; already correct                           |
| `vm/completion_record.rs`          | `CompletionRecord::consume`            | `Normal` \| `Return`→`Ok`, `Throw`→`Err`                             | ✅ Yes        | Non-generator callers; treats Normal and Return identically                       |

## Summary

- **15 sites** were audited across `vm/opcode/`, `builtins/generator/`, and the
  central `vm/mod.rs` handler functions.
- **13 sites** were already correct before this PR (they either delegate to a
  handler or emit `Throw` which was never inverted).
- **2 sites** needed fixing and were fixed in this PR:
  - `handle_yield`: `CompletionRecord::Return` → `CompletionRecord::Normal`
  - `handle_return`: `CompletionRecord::Normal` → `CompletionRecord::Return`
- The generator drive loop in `builtins/generator/mod.rs` was refactored into a
  shared `handle_resumption_result` to match the corrected semantics:
  `Normal`→SuspendedYield, `Return`→Completed, `Throw`→Completed.
