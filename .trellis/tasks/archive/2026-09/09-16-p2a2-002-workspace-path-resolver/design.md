# Design — P2A2-002 WorkspacePathResolver

## Boundary

`WorkspacePathResolver` consumes a server-created `WorkspaceLease` and a
single relative path string. It returns a filesystem target only after
containment validation. No public request parsing or caller migration belongs
to this task.

## Resolution

1. Reject lexical absolute, UNC, drive-prefixed, empty/root, and parent-path
   input before filesystem access.
2. Join the accepted path to `lease.canonical_root`.
3. Canonicalize the target, or walk upward to its nearest existing parent and
   canonicalize that parent when the target is absent.
4. Verify the canonical result is contained by the canonical Lease root using
   case-insensitive component comparison on Windows.
5. Return the joined target only after that validation. Uncertainty or I/O
   failure fails closed with the established `INVALID_PATH` family.

## Non-goals

No Source/Git/Media handler migration, public error-schema change, path-field
rename, traversal implementation, or new dependency.
