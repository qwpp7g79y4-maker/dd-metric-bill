# Contributing

Thanks for your interest in dd-metric-bill.

## Building and testing

```sh
cargo build
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

These checks must pass before a change is merged. Test fixtures must be synthetic: never add real metric samples, account data, or secrets.

## Commit messages

Commits follow [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/) in English, for example `fix: refuse a FIFO as a price file` or `docs: clarify the histogram factor`.

## Contact

PEPAI, pep_ai@icloud.com
