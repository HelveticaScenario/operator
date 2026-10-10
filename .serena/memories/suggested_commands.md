Commands:

- Run app: yarn start
- Build native N-API module: (cd crates/modular && yarn build) or yarn build-native
- Lint: yarn lint
- Typecheck: yarn typecheck
- Tests (Rust): cargo test
- Tests (renderer): yarn test
- Rebuild after Rust #[napi] changes: cd crates/modular && yarn build
