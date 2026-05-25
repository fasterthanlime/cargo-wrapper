# cargo-wrapper

Meant to live in your $PATH before `cargo`, so that it can call the "real" cargo
(well, the next cargo) while preventing stupid things like using `--package`.

The installed binary is named `cargo`. Put it in a directory that appears before
the real Cargo binary on `PATH`; it will find and run the next executable named
`cargo` after itself.

## Install

Install it into its own root, not into `~/.cargo/bin`:

```sh
cargo install --git https://github.com/fasterthanlime/cargo-wrapper \
  --root ~/.local/cargo-wrapper \
  --force
```

Then put the wrapper before the real Cargo on `PATH`:

```sh
export PATH="$HOME/.local/cargo-wrapper/bin:$PATH"
hash -r 2>/dev/null || true
```

Check the order:

```sh
which -a cargo
```

The first `cargo` should be `~/.local/cargo-wrapper/bin/cargo`. A later one
should be the real Cargo executable, usually the rustup shim in `~/.cargo/bin`.

For local development:

```sh
cargo install --path . --root ~/.local/cargo-wrapper --force
```

It rejects `cargo test` before forwarding the command, and points agents at
`cargo nextest run` instead. If the rejected test command also used `-p foo` or
`--package foo`, the diagnostic steers that selection to nextest's filter syntax:
`cargo nextest run -E 'package(foo)'`.

It also rejects `-p` and `--package` on other commands before forwarding them.
The point is not style policing: package selection changes the set of crates
being built, which changes the set of enabled features. The whole workspace
needs to establish one feature-unified build shape so Cargo stops invalidating
and overwriting useful cache entries with partial-workspace feature sets.
Keeping failures outside the selected crate visible is useful too, but the cache
invalidation is the main reason this wrapper exists.

Agents should run whole-workspace commands instead:

```sh
cargo check --workspace
cargo clippy --workspace -- -D warnings
cargo nextest run --workspace --no-fail-fast
```
