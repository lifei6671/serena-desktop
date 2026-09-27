# Source checkpoint 1

Mode CHILD_AGENT; target f18f8623540e8d5f8b4855d48a16a1bfab041cb9baca9e8a78c1859da39cdda5; 11/11 scope coverage COMPLETE/FRESH at review time. No writes/build/test/probe/network by reviewer. Source result CHANGES_REQUESTED, P0=0/P1=0/P2=1; production verification incomplete.

P2 ingress terminal acknowledgment gap: malformed $/cancel_request notification and matching response with error.code outside i32 pass local guard but are consumed/rejected before SDK custom Dispatcher, leaving inflight=true. Pending request times out instead of correctly classifying/ignoring input; idle connection stalls.

Repair round 1: boundary notification allowlist only session/update; unsupported notifications are counted/ignored without response. Validate full RawJsonRpcMessage using official public serde type before SDK admission, in addition to strict envelope gate. Regressions added for both examples, following response and EOF, and direct byte-stream backpressure. Original checkpoint is now STALE for protocol/client tests; final refreeze and independent re-review required.

Reviewer also confirmed: no unstable_protocol_v2 feature, so id-bearing _proxy/successor is passthrough and gets -32601 from original SDK Responder; no evidence of another source defect. Lock adds 10 packages, no existing version replacement/deletion.
