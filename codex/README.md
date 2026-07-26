# Codex tools

This repository is the home for personal Codex setup, plugins, and supporting
tools. It is also a local Codex marketplace.

## Included tools

- [`autocommit`](plugins/autocommit/README.md) batches changes from overlapping
  Codex sessions into automatic Git commits.
- [`succinct`](skills/succinct/SKILL.md) is a standalone, explicitly invoked
  skill that makes Codex's immediately previous response much shorter.
- [`pseudo`](skills/pseudo/SKILL.md) is a standalone, explicitly invoked skill
  that rewrites Codex's immediately previous response as JS-style pseudo-code.

## Install or update

Run the single repository installer:

```sh
./install
```

The installer is safe to run repeatedly. It rebuilds implementation artifacts,
replaces previously bundled copies, installs standalone skills, updates plugin
cachebusters, repairs the local marketplace registration when the repository
moves, and reinstalls every plugin listed in
`.agents/plugins/marketplace.json`.

Run it again after pulling or editing source code. Start a new Codex thread
afterward so the app loads the updated plugin snapshot.
