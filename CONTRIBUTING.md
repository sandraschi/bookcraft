# Contributing

Thanks for stopping by. This repo is a working craft, not a suggestion box:
small, tested, reviewed contributions beat grand redesigns every time.

## Contributor rules

1. **Small and focused.** Max 10 files per commit. One concern per pull
   request. If your change touches more, split it first and say so.
2. **Prove it works.** Paste test output (or a `run.json` excerpt) in the
   PR. A gate that reports success having examined nothing is a failure —
   show what was actually exercised: commands run, fixtures used, measured
   results (ms, bytes, rows).
3. **Docs travel with code.** Behavior change without a README / HELP /
   CHANGELOG touch is incomplete. New verbs need a help line; new scope
   needs a non-goals line.
4. **No fake greens.** Never assert what you did not run. Missing engine,
   token, or hardware = skip with a receipt (`DOWN`, `sim`), never a
   mocked pass dressed as green.
5. **No secrets, ever.** API keys, tokens, credentials do not belong in
   code, fixtures, logs, or screenshots. Redact before you commit.
6. **Respect boundaries.** This craft has documented neighbors and
   non-goals (see README Status section). If your change belongs in a
   neighboring craft, say so in the issue instead of building it here.
7. **English, plain words.** PRs and issues in English. No hype adjectives
   in user-facing text ("local-first, offline-capable" beats "sovereign").
8. **Lineage stays.** Pattern attributions (e.g. the Storytold craft-app
   harness) are load-bearing honesty, not decoration. Do not remove them.

## Issue rules

- **Bugs:** use the bug template. Repro steps + versions + logs, or it
  cannot be triaged. One bug per issue. Check open issues first.
- **Features:** use the feature template. Problem first, proposal second,
  non-goals mandatory. "Add X" without a problem statement will be sent
  back with questions.
- **Questions:** discussions, not issues — unless it ends in a docs fix,
  in which case file it as one.
- **Security:** do NOT file publicly. Contact the maintainer privately
  (see README for the current channel).
- No `+1` comments — use reactions. Maintainers close stale issues after
  a warning; reopen with new evidence, not complaints.

## Pull-request rules

- [ ] Linked issue (or a one-line reason there is none)
- [ ] Tests run + output pasted (or honest skip-with-receipt + why)
- [ ] Docs touched (README / HELP / CHANGELOG as applicable)
- [ ] One concern, ≤10 files, no drive-by reformatting
- [ ] Destructive or mutating behavior behind an explicit confirm gate
      (or a written reason why it needs none)
- [ ] CI green (or the red is explained in the PR body, not ignored)
- Mark draft until reviewable. Re-request review when ready, don't @-ping.
  Squash on merge unless the history itself is the documentation.
