# alias_anon_junkpad — 360 alias with junk-padded one-byte nodes

`ALIAS_360` is the author's 360 alias re-saved on the console on 2026-10-09
after switching only "Turn indicators" On, passed through
`docs/re/anonymize_alias.py` with the name `ANONYMOUS 1` (same caveats as
`docs/re/alias_anon/README.md`: STFS hashes and signatures stale, not
console-loadable).

Why it is tracked: it is the only sample whose on-chain one-byte property
nodes carry 360 heap junk in their pad bytes (and once in the flag word):

- GameplaySettings PC 0x30: `01 00 13 10` (value 1)
- PlayerSettings3 PC 0x30: `01 00 00 5d` (value 1), flag word `00001b10`
- GStatsImpl::SavableStats PC 0xdc: `00 00 00 08` (value 0)

The value is the first byte; the pad is junk. The old rule (pad must be zero)
u32-swapped these, so the PC read 0x10 / 0x5d.

This particular save did not yet carry the "Turn indicators" change; later
360 saves did (PlayerSettings0 node 32 at PC 0x200, 1 = On, same as on PC;
node 31 = leaderboard). See docs/re/FORMAT-NOTES.md (PlayerSettings0).
