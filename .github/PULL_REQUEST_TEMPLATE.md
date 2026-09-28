## Summary

<!-- What does this change, and why? Link issues with "Closes #123". -->

## Checklist

- [ ] I read [`AGENTS.md`](../AGENTS.md) and match the surrounding style.
- [ ] If runtime behavior changed, I bumped `version` in `Cargo.toml` and
      included the updated `Cargo.lock`.
- [ ] I ran the full gate locally:

  ```sh
  cargo fmt --check
  cargo clippy --locked --all-targets -- -D warnings
  cargo test --locked
  cargo build --release --locked --bin acc
  python tests/smoke.py
  python tests/runtime.py
  ```

- [ ] New work stays off the speech/UI hot path, or the trade-off is explained.
- [ ] No credentials, private transcripts, or raw audio are logged or committed.

## Notes for reviewers

<!-- Platform tested, hardware that still needs validation, follow-ups, etc. -->
