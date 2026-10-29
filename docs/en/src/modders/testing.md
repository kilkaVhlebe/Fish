# Testing mods

`cargo fish test` builds the mod and runs every `tests/*.toml` script in the in-process test host
(`fish-test-host`): a real loader in a temporary game directory with the simulated client. Each script gets
a fresh host. A failure prints the first failed step with *expected* and *actual*:

```text
FAILED  hello.toml
        step 1: expect.logs failed
          expected: a message containing "this is never logged"
          actual:   ["Fish 0.1.0 (ABI 0.1) found 1 mod candidate(s)", …]
```

## Script schema v1

```toml
name = "optional display name"

[[step]]
key = { name = "H", pressed = true }   # key event
command = "fish hello"                 # client command (no leading slash)
chat = "hi"                            # chat the player sends
ticks = 20                             # advance client ticks
frames = 2                             # render frames

[step.expect]
logs = ["Hello from main"]             # substrings among all logged messages
chat = []                              # substrings among chat shown to the player
hud = []                               # substrings among text drawn on the HUD
command_feedback = []                  # substrings among command feedback
stopped = ["panicky"]                  # mods that must be in the failed state
```

Unknown keys are errors. Within a step the actions run in the order `key`, `command`, `chat`, `ticks`,
`frames`; then `expect` is checked. `logs` and `stopped` observe the real loader; the other
observations (`chat`, `hud`, `command_feedback`) are fed by the client hooks of later phases, so until a mod
produces such output they report `nothing`. Later phases append keys (for example world interaction).

## From Rust tests

`fish_test_host::TestHost` is the same host as a library: `build_example`-style packing, `boot`, `fire` for
events, `share` to read the object share, `mod_log(id)`, `state(id)`, `reboot()`. The Fish repository's own
loader tests are written with it.
