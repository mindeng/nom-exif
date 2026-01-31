# Usage of afl-fuzz

Refer to [AFL Tutorial](https://rust-fuzz.github.io/book/afl/tutorial.html).

## Install *cargo-afl*

```sh
cargo install cargo-afl
```

Upgrade:

```sh
cargo install --force cargo-afl
```

## Build & run fuzz testing

```sh
cd afl-fuzz/
cargo afl build --release
cd ..
cargo afl fuzz -i ./testdata -o out target/release/afl-fuzz
```

## Reproduce a crash

```sh
cargo afl run ./target/release/afl-fuzz < out/default/crashes/[SAVED_CRASH_FILE]
```

## System Config Issues

❯ cargo afl fuzz -i ../testdata/ -o out target/release/afl-fuzz
afl-fuzz++4.36a based on afl by Michal Zalewski and a large online community
[+] AFL++ is maintained by Marc "van Hauser" Heuse, Dominik Maier, Andrea Fioraldi and Heiko "hexcoder" Eißfeldt
[+] AFL++ is open source, get it at <https://github.com/AFLplusplus/AFLplusplus>
[+] NOTE: AFL++ >= v3 has changed defaults and behaviours - see README.md
[+] No -M/-S set, autoconfiguring for "-S default"
[*] Getting to work...
[+] Using exploration-based constant power schedule (EXPLORE)
[+] CmpLog level: 2
[+] Enabled testcache with 50 MB
[+] Generating fuzz data with a length of min=1 max=1048576

[-] Whoops, your system is configured to forward crash notifications to an
    external crash reporting utility. This will cause issues due to the
    extended delay between the fuzzed binary malfunctioning and this fact
    being relayed to the fuzzer via the standard waitpid() API.

    To avoid having crashes misinterpreted as timeouts, please run the
    following commands:

    SL=/System/Library; PL=com.apple.ReportCrash
    launchctl unload -w ${SL}/LaunchAgents/${PL}.plist
    sudo launchctl unload -w ${SL}/LaunchDaemons/${PL}.Root.plist

[-] PROGRAM ABORT : Crash reporter detected
         Location : check_crash_handling(), src/afl-fuzz-init.c:2618

If you see an error message like `shmget() failed` above, try running the following command:

    cargo afl system-config

Note: You might be prompted to enter your password as root privileges are required and hence sudo is run within this command.

❯ cargo afl system-config
Running: "sudo" "--reset-timestamp" "~/.local/share/afl.rs/rustc-1.92.0-ded5c06/afl.rs-0.17.1/afl/bin/afl-system-config"
Password:
This reconfigures the system to have a better fuzzing performance.
WARNING: this reduces the security of the system!

kern.sysv.shmmax: 4194304 -> 524288000
kern.sysv.shmmin: 1 -> 1
kern.sysv.shmseg: 8 -> 48
kern.sysv.shmall: 1024 -> 131072000
Settings applied.

Unloading the default crash reporter

It is recommended to disable System Integrity Protection for increased performance.
See: <https://developer.apple.com/documentation/security/disabling_and_enabling_system_integrity_protection>
