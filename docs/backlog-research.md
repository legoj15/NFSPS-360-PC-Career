# Open research backlog (post-1.1.0)

Hand-off outline for a fresh session. None of this blocks users: every
converted save tested so far loads and plays correctly in-game. These are
values the converter copies or resets without proof that the result is the
PC's native meaning. Start with `docs/HANDOFF.md` and `docs/re/` (the
README, FORMAT-NOTES and oracle pairs); the Python converter in
`scripts/python/nfssave/convert.py` is the spec, Rust and PowerShell must
stay byte-exact with it.

Rules: tests first; any finding that changes output lands in all three
ports with updated goldens and a justification; in-game confirmation by the
user is the final check for anything save-visible.

## 1. Save fields never proven against a native PC/360 pair
No matching save pair existed for these, so they are converted by the
general rules and never compared:
- Blueprint sets: per-set ints `+0x224..0x240`, set words `+0x448` / `+0x568`,
  float block from `+0x738`, record tail `+0x171C..0x1870`.
- Car DB per-car 0x40-byte entries at `0x7C980`: byte 0 is `00` after
  conversion, `ff` on native PC saves.
- One Python setting is written as a range but used as two single
  positions; check it is not a latent off-by-range bug (grep the
  struct-fix offset tables in convert.py).
Approach: get a PC save and a 360 save with the same car/blueprint state
(or a PC save touched in-game after conversion) and diff those offsets.
If the game rewrites a value on its own re-save, record the native form.

## 2. 360 memory filler in AudioSettings
Alias `AudioSettings` node 8 carries `0xAAAAAAAA` (360 uninitialised heap
fill) as its f32 (about -3e-13); a native PC alias holds 0. It has no
visible effect so far. Find out which audio option node 8 is (toggle audio
options on PC, diff the re-save), then decide: reset to the PC default on
convert, or leave it.

## 3. The 12 unexplained race-day progress entries
GameplayData's race-day progress table (90 x `[key][state][score]`, first
key `0xA70EA9B0`, last `0xFA5D360A`). `CONSOLE_ONLY_RACEDAYS` in convert.py
resets 17 keys that the 360 always marks and the PC never writes; 5 of
them are the custom race-day slots. The meaning of the other 12 keys'
360 state is unknown. It was ruled out as the Race Day crash cause, and
Race Day works after conversion. Investigate: which race days those keys
are (game data / FE strings), whether they are 360-only content, and
whether resetting them hides any progress the player earned.

## Parked (wait for evidence, do not start)
- FATX raw-drive scanner never run on real media: wait for a user report;
  no FATX hardware available.
- Latent conversion rule with no sample to test against: the one-byte
  settings rule (`scalar_tail` still requires a zero pad, unlike
  `fix_node_flags`, which tolerates 360 heap junk there). Revisit only if a
  save of that shape turns up.
- `docs/re/fieldmaps/` positional maps: superseded by the node grammar + flag
  fix (FORMAT-NOTES), yet `payload_rules` still embeds them. Check whether
  removing them changes any golden before deleting.
