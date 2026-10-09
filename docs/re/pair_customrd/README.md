# pair_customrd — PC-written custom race day

`CAREER_aa_after_pc`: what nfs.exe (PC) wrote after the user created one
custom race day ("My Race Day 1", 3 events) on top of the converted fresh
CAREER_02 (2026-10-09). The game saved it under the stray name `CAREER_ª`
because the converted alias was not loading at the time (fixed: alias extra
word 1). Oracle for the CustomRaceDayMemcard layout: the PC writes the same
node layout as the 360.

Anonymized: the 12 MAC hex characters at the start of the race-day GUID
string (file offset 0xA67F8) are zeroed. Nothing else changed; MC02 CRCs are
therefore stale (the tests parse the tree only). The untouched original is in
the user's `SAVE/SaveConverter backups/2026-10-09_strays/CAREER_ª/`.
