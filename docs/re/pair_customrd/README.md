# pair_customrd — PC-written custom race day

`CAREER_aa_after_pc`: what nfs.exe (PC) wrote after the user created one
custom race day ("My Race Day 1", 3 events) on top of the converted fresh
CAREER_02 (2026-10-09). The game saved it under the stray name `CAREER_ª`
because the converter u32-swapped the FECareer career-slot name node (the
360 name "01" + NUL + 0xAA fill, swapped, began with the fill byte; fixed
by fix_career_name, d58ec2b). An earlier note blamed the alias not loading (alias extra word 1,
a separate bug, also fixed). Oracle for the CustomRaceDayMemcard layout: the
PC writes the same node layout as the 360 (layout in
docs/re/FORMAT-NOTES.md, "In-game findings").

Anonymized: the 12 MAC hex characters at the start of the race-day GUID
string (file offset 0xA67F8) are zeroed. Nothing else changed; MC02 CRCs are
therefore stale (the tests parse the tree only). The untouched original is in
the user's `SAVE/SaveConverter backups/2026-10-09_strays/CAREER_ª/`.
