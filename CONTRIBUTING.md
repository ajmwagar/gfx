# Contributing

Contributions are welcome. Keep the library render-only: windowing, input,
application state, and domain widgets belong in consumers.

Before opening a pull request, run:

```sh
cargo fmt --all -- --check
cargo check --all-targets
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
```

Performance changes should include a benchmark or an explanation of the
measured GPU/CPU effect. Public interfaces and serialized theme fields require
tests because they are compatibility boundaries.

By contributing, you agree that your work may be distributed under the
project's MIT OR Apache-2.0 license.

