# LT AI Coach Drafter

A Teamfight Manager 2 mod that lets the LT AI Coach app draft for you in game:
the draft screen's "Delegate to Head Coach" becomes "Delegate ban/pick to LT Coach", and the optional
swap-stage Coach button applies LT Coach's arrangement. Tested on game 0.6.2; the swap-stage patch is reviewed for 0.6.2 and 0.6.3 (see `src/swap.rs`, `PROFILES`).

See `README.txt` for behaviour, safety rules and the swap-stage native patch.

## Build

The mod uses the game's stable mod SDK (`mod-sdk-stable-0.6.0`), which is **not** included here.
Put it next to this folder so `../mod-sdk-stable-0.6.0/mod-api-stable` exists, then:

```
cargo build --release
```

Copy `target/release/lt_ai_coach_drafter.dll` and `mod.mod_info` into the game's `mods/lt_ai_coach_drafter/`.

`cargo test` runs without the game or the Coach app (tests that need them skip themselves).
`tests/fixtures/*.tsv` are shared with the Coach app, which must produce identical fingerprints.

## License

Public domain ([Unlicense](LICENSE)). Do anything you like with it.
