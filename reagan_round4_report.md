# Reagan Round 4 Report

## Reindex wall-clock

- Cold reindex before: `real 6.74s` (`rm ~/.config/asm/index.db && /usr/bin/time -p target/release/asm reindex`, pre-change)
- Cold reindex after: `real 0.48s` (`rm ~/.config/asm/index.db && /usr/bin/time -p target/release/asm reindex`, post-change)
- Warm reindex runs after cold: `0.01s, 0.01s, 0.01s, 0.01s, 0.01s`
- Warm median across 5 runs: `0.01s` / `10ms`

## Validation

- `cargo build --release`: pass
- `cargo test`: pass, 59 tests
- `cargo clippy --all-targets -- -D warnings`: pass

## Test updates

- None. Existing FTS/index tests stayed green against the preserved synchronous helper API.
