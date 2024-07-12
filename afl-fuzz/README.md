# Usage of afl-fuzz

## Install *cargo-afl*

```shell
cargo install cargo-afl
```

## Build & run fuzz testing

```shell
cd afl-fuzz/
cargo afl build --release
cargo afl fuzz -i ../testdata/ -o out target/release/afl-fuzz
```

## Reproduce a crash

```shell
cargo afl run ./target/release/afl-fuzz < out/default/crashes/[SAVED_CRASH_FILE]
```
