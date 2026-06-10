## Comments
* Comments should be written for FUTURE readers to give context that is not obvious
from the code alone.
* Comments should NEVER include context directly referencing your current task.
* Prefer complete sentences with punctuation
* Guidelines:
    * Comments should not duplicate the code.
    * Good comments do not excuse unclear code.
    * If you can't write a clear comment, there may be a problem with the code.
    * Comments should dispel confusion, not cause it.
    * Explain unidiomatic code in comments.
    * Provide links to the original source of copied code.
    * Include links to external references where they will be most helpful.
    * Add comments when fixing bugs.
    * Use comments to mark incomplete implementations.

## Commit messages
* Limit the subject line to 50 characters
* Capitalize the subject/description line
* Do not end the subject line with a period
* Separate the subject from the body with a blank line
* Wrap the body at 72 characters
* Use the body to explain what and why
* Use the imperative mood in the subject line let it seem like you’re giving a command eg “feat: Add unit tests for user authentication”. Using the imperative mood in commit messages makes them more consistent and commands-like, which is helpful in understanding the actions taken.
* NEVER include details that are only relevant DURING the change, like discarded choices.

## Unit Tests
* Require 60% test coverage.

## Rust Best Practices
Distilled from the Rust Design Patterns book. Rules only.

### Idioms — do
* Take borrowed types in arguments (`&str`, `&[T]`, `&Path`), not `&String`/`&Vec<T>`/`&PathBuf`.
* Concatenate strings with `format!` when readability matters; reach for `push_str`/`write!` only in hot paths.
* Provide an associated `new()` constructor by convention.
* Implement `Default` (derive when possible) so types work in generic and `..Default::default()` contexts.
* Implement `Deref`/`DerefMut` on collection wrappers to expose a borrowed view instead of re-forwarding methods.
* Put cleanup in a `Drop` impl so it runs on every exit path, including panics and early returns.
* Use `mem::take` / `mem::replace` to move owned values out of `&mut` (e.g. swapping enum variants) without cloning.
* Defer conditional init with `&mut dyn Trait` for on-stack dynamic dispatch — no `Box`, no monomorphization bloat.
* Iterate over an `Option` directly via its `IntoIterator` impl (`.into_iter()`, `.extend()`, `.chain()`) instead of `if let`.
* Selectively `move`/clone/borrow into a closure by rebinding inside a nested scope before the closure.
* Mark evolvable public enums/structs `#[non_exhaustive]`, or add a private field, to keep adding variants/fields non-breaking.
* Factor doctest setup into a helper function that takes the value, keeping examples free of boilerplate.
* Use temporary mutability: build with `let mut`, then rebind to an immutable `let` to signal "done mutating".
* On a fallible operation that consumed an argument, return that argument inside the error so callers can retry without cloning.

### Design patterns
* **Newtype** — wrap a type in a tuple struct for type safety, encapsulation, or to add foreign trait impls.
* **RAII guards** — tie resource acquisition to construction and release to `Drop`; hand out guard objects.
* **Strategy** — express interchangeable algorithms as impls of a shared trait, selectable at compile or run time.
* **Command** — reify actions as objects/closures to queue, log, or undo them.
* **Visitor** — separate an algorithm from the heterogeneous data it traverses.
* **Interpreter** — define a small DSL plus interpreter for recurring, expressible problems.
* **Builder** — construct many-optioned objects via chained methods returning a finished value.
* **Fold** — produce a transformed copy of a (often recursive) structure by visiting each node.
* **Compose structs** — split a large struct into smaller ones so the borrow checker can borrow disjoint parts independently.
* **Prefer small, focused crates** — they compile in parallel, are easier to reuse, and sharpen API boundaries.
* **Contain unsafety** — isolate `unsafe` in the smallest possible module behind a safe API, with invariants documented.
* **Custom trait for bounds** — bundle complex `where` bounds into one custom trait (with a blanket impl) to keep signatures readable.

### FFI
* Map errors across the boundary: flat enums → integer codes, rich errors → code plus message, custom types → C-friendly reprs.
* Accept foreign strings borrowed as `&CStr`; don't copy into Rust ownership.
* When passing strings out, bind the `CString` to a variable first so it outlives the pointer (avoid dangling).
* Expose object-based APIs where encapsulated types are owned by Rust, managed by the user through functions, and opaque (pointers only).
* Consolidate related Rust types behind a single wrapper type at the FFI surface.

### Anti-patterns — avoid
* Cloning just to satisfy the borrow checker — restructure or use `mem::take`/`replace` instead.
* Crate-wide `#![deny(warnings)]` — it makes builds brittle across toolchains; deny specific lints instead.
* `Deref` polymorphism — don't fake inheritance via `Deref`; use explicit conversions/traits.

### Principles
* Favor generics + traits as type classes for zero-cost, compile-time polymorphism.
* Apply SOLID, DRY, KISS, and YAGNI; Rust's type system often removes the need for classic OO patterns.
