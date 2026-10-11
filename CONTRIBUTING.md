# Contributing

Thanks for your interest in dd-metric-bill.

## Building and testing

```sh
cargo build
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Some tests check the JSON output with `python3` (standard library only), so Python 3 must be on your `PATH`. These checks must pass before a change is merged. Test fixtures must be synthetic: never add real metric samples, account data, or secrets.

## Issues

Use the [issue forms](https://github.com/qwpp7g79y4-maker/dd-metric-bill/issues/new/choose) for bugs and feature requests. Remove real host names, IDs and other sensitive data from any sample input before you paste it. Report security problems privately as described in [SECURITY.md](SECURITY.md).

## Commit messages

Commits follow [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/) in English, for example `fix: refuse a FIFO as a price file` or `docs: clarify the histogram factor`.

## Contact

PEPAI, pep_ai@icloud.com
