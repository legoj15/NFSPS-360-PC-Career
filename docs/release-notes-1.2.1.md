## Improved
- Added an internal safety check while fixing the garage (car database)
  part of a save: if an unexpected internal error ever happened, the
  converter now stops with a clear error instead of risking a damaged
  save file.

Converted saves are unchanged from 1.2.0 — byte for byte. If version
1.2.0 already worked for you, there is nothing to do and no need to
convert your saves again.
