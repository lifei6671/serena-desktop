# Frozen design

Store intent -> original Prompt owner -> typed cancel -> physical flush -> InterruptAck -> bounded exact terminal -> existing staged result -> whole Job shutdown -> approved evidence -> atomic release.

Pre-MarkSent intent fails closed. Accepted prompt flush must precede cancel. Exact response takes priority over pending cancel. Deadline absolute from cancel flush; no mutex across await. Error marks Sent uncertain, never terminal. No client registry or schema.

Review repair: exact Prompt response now commits optional provider request identity and ObserveTerminal in one IMMEDIATE Store transaction. It reads current generic ownership inside that transaction, permits only Running/CancelRequested/Cancelling with dispatched original Runtime and no staged generic terminal, and compares the entire expected private snapshot/revision. Ordinary generic/private OCC APIs remain unchanged. No terminal retry or Claim mutation is added.
