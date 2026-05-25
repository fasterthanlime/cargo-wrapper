# cargo-wrapper

Meant to live in your $PATH before `cargo`, so that it can call the "real" cargo
(well, the next cargo) while preventing stupid things like using `--package`.

The installed binary is named `cargo`. Put it in a directory that appears before
the real Cargo binary on `PATH`; it will find and run the next executable named
`cargo` after itself.

It rejects `-p` and `--package` before forwarding the command. The point is not
style policing: package selection changes the shape of a workspace command,
hides failures outside the selected crate, and makes Cargo throw away the cache
shape a full workspace build would have reused.

Agents should run whole-workspace commands instead:

```sh
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --workspace --no-fail-fast
```
