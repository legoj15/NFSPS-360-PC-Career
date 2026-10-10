# alias_anon — anonymized alias golden fixture

`ALIAS_360` is the author's 360 alias save (`Extracted/Alias/ALIAS_360`,
gitignored) passed through `docs/re/anonymize_alias.py` with the name
`ANONYMOUS 1`. Container (STFS file-table) name: `ALIAS_ANONYMOUS 1`.

Regenerate (needs the personal save):

    python docs/re/anonymize_alias.py Extracted/Alias/ALIAS_360 docs/re/alias_anon/ALIAS_360 "ANONYMOUS 1"

The output is byte-deterministic. Converted output (md5
`377651916f0e1bd488561b7481a66da1`) differs from the personal golden
(`a5a24e0f...`, byte-identical to the alias verified in-game on 2026-10-09)
only in the MC02 CRC words, the extra-blob name, the PC tree hash and the
UserProfile name.

Changed vs the source: player name in file table, CON display name, MC02 extra
blob and UserProfile; CON certificate body, console id, profile id and device
id zeroed; MC02 CRCs recomputed. STFS hash tables, header SHA-1, signatures
and the 360 tree hash are stale — the converter verifies none of them, but a
console will not load this package.

Covers alias-only converter paths: 64-byte extra blob, PCControllerSettings
native default bindings, alias fieldmap rules.
