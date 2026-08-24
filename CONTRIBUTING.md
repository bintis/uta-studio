# Contributing to Uta! Studio

Thanks for helping improve Uta! Studio.

## Workflow

1. Open an issue or discussion at
   [github.com/bintis/uta-studio](https://github.com/bintis/uta-studio) for
   user-facing features or architecture changes.
2. Keep pull requests focused and explain the user-visible outcome.
3. Add or update tests for changed behavior.
4. Run the relevant checks from `docs/engineering-constraints.md`.
5. Update `CHANGELOG.md` for user-visible releases.

Small documentation, test, and clearly scoped bug fixes may go directly to a
pull request.

## Safety

- Never commit user media, model weights, caches, credentials, build output, or
  generated runtime environments.
- Tests must use isolated fixtures and must not mutate a user's library,
  settings, models, or analysis cache.
- Model downloads and runtime installation must remain explicit user actions.

## License

Contributions are provided under GPL-3.0.
