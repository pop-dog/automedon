# Coding Standards
## Documentation
* Aggressively prune stale documentation.
* Code should be self-documenting via good code and comment hygiene. Docs should be written at a level of abstraction that code refactors do not make the docs stale.
* Most design should live in Pull Requests outside of the source code. In-source design should be reserved for guiding future development. If it's ambiguous, bias towards pruning.

## Comments
* Use complete sentences.
* Be precise and concise. Use large doc comments sparingly.
* Describe "why", not "what" or "how". The code MUST speak for itself.
* Write for a future audience. NEVER use comparative language to reference the state before this change.
* A comment MUST add information the annotated code cannot convey on its own. If the symbol name, signature, or an adjacent declaration already states it, delete the comment.
* NEVER restate a value or behavior that appears verbatim nearby — a routing table, an enum, a config field, or a called script's own header. Duplicated facts drift out of sync.
* Let the named artifact speak for itself. Do not narrate a step, function, or call whose name and body already say what it does; comment only the non-obvious (a rationale, a constraint, or a magic number's derivation).
* Do not annotate every element for symmetry. Most lines need no comment; add one only where there is genuinely something a future reader would otherwise miss.

## Commit Messages
* Capitalized subject line ≤ 50 characters.
* Body lines ≤ 72 characters.
* Use imperative case.
* Write for a future audience.
* Detailed accounting of decisions, alternatives considered, etc. should live in the Pull Request, not the commit.

### Conventional Commits
1. Commits MUST be prefixed with a type, which consists of a noun, feat, fix, etc., followed by a colon and a space.
2. The type feat MUST be used when a commit adds a new feature to your application or library.
3. The type fix MUST be used when a commit represents a bug fix for your application.
4. An optional scope MAY be provided after a type. A scope is a phrase describing a section of the codebase enclosed in parenthesis, e.g., fix(parser):
5. A description MUST immediately follow the type/scope prefix. The description is a short description of the code changes, e.g., fix: array parsing issue when multiple spaces were contained in string.
6. A longer commit body MAY be provided after the short description, providing additional contextual information about the code changes. The body MUST begin one blank line after the description.
7. A footer MAY be provided one blank line after the body (or after the description if body is missing). The footer SHOULD contain additional issue references about the code changes (such as the issues it fixes, e.g.,Fixes #13).
8. Breaking changes MUST be indicated at the very beginning of the footer or body section of a commit. A breaking change MUST consist of the uppercase text BREAKING CHANGE, followed by a colon and a space.
9. A description MUST be provided after the BREAKING CHANGE: , describing what has changed about the API, e.g., BREAKING CHANGE: environment variables now take precedence over config files.
10. The footer MUST only contain BREAKING CHANGE, external links, issue references, and other meta-information.
11. Types other than feat and fix MAY be used in your commit messages.

## Pull Requests
* Follow a structure of "Background", "Why", "Approach", "Testing".
* **Background.** Establish a foundation to frame the change.
* **Why.** Justify the change. What problem is this solving?
* **Approach.** How does this change solve the problem?
    - **Alternatives Considered (Optional).** If other solutions were considered and discarded, document them here.
* **Visual aids (Optional).** Include Mermaid diagrams and Markdown tables when they help illustrate the change. For example, if there are non-obvious relationships between
modules, or the change involves a process flow, a Mermaid diagram is appropriate.
* **Testing.** How was this change testing? Does this change introduce new tests?
* **Compatibility (Optional).** Does this introduce any backwards-incompatible changes?


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
