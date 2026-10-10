# Delegation log

One line per delegation: what was delegated, to whom, outcome, escalations.
It tunes the model ladder; append new lines at the bottom, newest last.
Moved here from docs/HANDOFF.md on 2026-10-10 (that file keeps only state).

Lanes: `shop26` = Qwen 27B (opencode, free lab lane; slow, 6-45 min);
`glm-flash` = GLM-5.3-flash (opencode); `Haiku` = the `reviewer`/`scout`
agents; `impl` = Sonnet medium. "Orchestrator" = the session model itself.

| Date | Work | Agent / lane | Outcome | Escalations |
|---|---|---|---|---|
| 2026-10-04 | STFS reader, flag words, car slots, MD5 (all RE + fixes) | orchestrator | in-game verified | none |
| 2026-10-05 | FAT32 USB scan + FATX button + backup-on-replace, review round 1 | shop26 + glm-flash | 1 shared medium (FATX button mid-conversion) fixed, plus lows fixed (unbounded reads of save-named files, unsorted FATX-mode report); 3 rejected (UAC on UI thread, signed-HINSTANCE check, GetLogicalDrives==0) | none |
| 2026-10-05 | same, review round 2 | shop26 + glm-flash | 1 shared medium (cross-run overwrite -> backup-then-replace, user decision) + lows fixed | none |
| 2026-10-05 | same, review round 3 | shop26 + glm-flash | GLM high confirmed (backup keyed on input name, not CON name) fixed | none |
| 2026-10-06 | fatx TITLE-name relabel review | shop26 | approve, 1 low fixed | none |
| 2026-10-06 | anonymized alias fixture review | shop26, glm-flash | shop26 1 medium REJECTED (display-name offset claim was wrong); glm-flash partial (sandbox auto-reject), no findings | none |
| 2026-10-06 | PowerShell port of the converter | impl | ok first try; correctly stopped on my wrong test expectation | 0 |
| 2026-10-06 | PS port review (4dcbc3e..77bd8d9) | shop26 + glm-flash | follow-up commit; 2 rejected (probed: Join-Path drive-relative claims) | none |
| 2026-10-06 | Python CLI fixes review | shop26 + glm-flash low | 11 lows, fixed; 1 rejected | none |
| 2026-10-06 | PowerShell CLI redesign port | impl | ok first try | 0 |
| 2026-10-06 | CLI redesign review | shop26 + glm-flash high | fixed; Qwen-high "unary comma breaks multi-save folders" REJECTED (probed, idiom is right) | none |
| 2026-10-06 | no-console GUI subsystem review | shop26 + glm-flash | 3 lows fixed; Qwen-high "PE32+ Subsystem at 64" + 1 low rejected | none |
| 2026-10-09 | alias options (len-1 nodes, tail words) review | Haiku reviewer; shop26 + glm-flash high | pass with nits; no parity bugs; u8 rule tightened, tests added | none |
| 2026-10-09 | Race Day crash fix: Rust + PS ports (2 rounds) | impl x2 | ok first try both rounds; twice used Python replace scripts on Rust text (once aborted and redone with Edit, once on non-literal Rust text) | 0 |
| 2026-10-09 | race-day crash review | shop26 (timed out, no findings), glm-flash | 10 lows + 1 medium; 1 rejected ("[0][len] scan reverses to BE") | none |
| 2026-10-09 | PS short-record clamping | orchestrator; review shop26 (45 min timeout), glm-flash | parity confirmed all lengths; 1 low deferred (later fixed) | none |
| 2026-10-09 | PS RehashGameplay short payload | orchestrator; shop26 6 min | parity confirmed; 1 high REJECTED ("GetField cannot see a C# const") | none |
| 2026-10-09 | twin-recovered records via convert_to_pc_record | orchestrator; shop26 11 min | approve | none |
| 2026-10-09 | alias-options branch merge | orchestrator; shop26 approve, glm-flash | no parity bugs; PS scalar_tail vector added | none |
| 2026-10-09 | career-name / RaceData / PCController fixes review | glm-flash; shop26 errored after 66 s, no output (lane problem, not retried) | no code findings; 5 stale doc/comment items fixed | none |
| 2026-10-10 | backlog facts | scout (Haiku) | ok; one stale claim caught (dry-run folder nit already fixed) | 0 |
| 2026-10-10 | twin removal + Python dry-run name rule | impl | ok first try | 0 |
| 2026-10-10 | internal-gap spill fix, 3 ports | impl | ok first try | 0 |
| 2026-10-10 | one save-name rule + order, 3 ports | impl | ok first try | 0 |
| 2026-10-10 | review 1302160..75e9029 | Haiku + shop26 + glm-flash | refusal-order and dead-test-param fixed; app backup-before-name fixed; eaf7a15 Haiku nit wrong (convert_to_pc_record is in place); glm-flash caught missing PS trailing pin | none |
| 2026-10-10 | ad24aa1 / 71199fb / 8cf2f9b reviews | ad24aa1 and 8cf2f9b: Haiku + shop26 + glm-flash; 71199fb: shop26 alone | app backup-before-corruption documented as a user call (ad24aa1); library convert_one order fixed; vacuous PS assertions fixed (71199fb); Haiku's "refusal hides the backup" wrong; shop26's "rename onto a folder moves the file into it" refuted by a new test (8cf2f9b) | none |
| 2026-10-10 | prepare/backup/write order (#10) | orchestrator (small, serial) | done | none |
| 2026-10-10 | outside review of 062aa43..1723bee + 5a0402c | Haiku reviewer (8 min), glm-flash (17 min), shop26 (36 min) | Haiku: 10 findings, 1 real (file-table block length), 3 stale or wrong per the session note (its rejected claims list four), complaint logged; glm-flash: 2 real (hostile tree: Rust panic, 2 GiB alloc) + 1 agreed; shop26 approve, 0 new | none |
| 2026-10-10 | spot check 7f1b310..7c35c1d | shop26 (16 min) | approve; 1 low (short blobs) fixed | none |
| 2026-10-10 | handoff retirement: gap analysis (16 sections, extractor then checker) | scout + reviewer (Haiku) workflow, 32 agents | ~65 facts held only by the old handoff, 27 confirmed-open items, several stale claims (a false "CI-tested", an 11-vs-12 arithmetic error) | none |
| 2026-10-10 | handoff retirement: coverage + accuracy audit of the new docs | reviewer (Haiku) workflow, 95 agents (each problem re-checked by a second agent) | 42 of 72 reported problems confirmed and fixed (10 lost facts, 32 inaccurate or unsupported claims, several of them old errors in FORMAT-NOTES); 30 rejected | none |
| 2026-10-10 | handoff retirement: second opinion on the final docs | shop26 (22 min), glm-flash (15 min) | shop26 request-changes: 1 medium REJECTED (test counts inferred from commit deltas, measured numbers were right), 2 lows fixed (stray-file caveat, UserProfile 0x2848), 1 low dropped by user decision; glm-flash found no lost facts and 4 lows (banner overclaim, "all four", 2 stale line cites), all fixed | none |
| 2026-10-10 | handoff retirement: writing the docs, cleanup | orchestrator (serial, tightly scoped) | done | none |
| 2026-10-10 | open-work items 1-10 (three ports, tests first) | orchestrator (serial, cross-port, context already loaded) | done; found 2 real PowerShell bugs (folder at target got the .tmp moved in; read-only save overwritten) | none |
| 2026-10-10 | open-work review gate | Haiku `reviewer` (6 min) | PASS WITH NITS; confirmed and fixed: superscript device names, PS culture StartsWith, renamed misleading test, vacuous assert, wrong Win11 rationale; rejected: empty CON name (parser refuses it); headless `--out` parity raised as a user decision | none |
| 2026-10-10 | open-work second opinion | shop26 (Qwen 27B), glm-flash | both "comment": agreed on duplicate-name wording drift (fixed, all ports pinned); shop26 also PS "Exception calling" wrapper (fixed) and a stale docstring (fixed); glm-flash PS self-check test gap (logged); both flagged astral-plane sort order (ignored, hand-made names only). shop26 noticed the tree change mid-review and said so | none |

## Ladder observations (so far)
- `impl` (Sonnet medium): 7 dispatches (PS port, PS CLI, race-day Rust + PS
  x2, twin removal, gap bugs, name-rule parity), every one accepted first
  try, no escalations. Weak spot: Python replace scripts on Rust or
  backslash text (see memory note no-python-for-rust-strings).
- Haiku `reviewer` / `scout`: at least 3 of 10 findings in the 2026-10-10
  core review were stale or wrong (stale branch state, misread code), and it
  raised a wrong nit in at least two other reviews; useful as a cheap first gate,
  never as the only gate. Always verify before acting on its findings.
- shop26 (Qwen 27B): slowest lane (6-45 min when it finishes); three times it timed out
  or errored with no findings (2026-10-09). When it does return, it is the
  most reliable approver.
- glm-flash: best at hostile-input and parity edge cases (found the Rust
  panic and the 2 GiB allocation); its "high" findings still need probing.
