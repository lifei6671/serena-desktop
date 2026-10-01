# CB7-005 Design

Authority: current user request (all ten sections), implementation-task-breakdown CB7-005; technical-design §15.1, §21/21.1, §23, §31; CB6-002/004/005 and CB7-002/003/004.

Flow: registered resolved LaunchSpec -> frozen Execution/workspace/R1 prepare -> private MarkSent durable -> accepted -> generic Dispatching durable -> SDK-generated exact session/prompt frame write and inner.flush -> Dispatched -> Running -> exact private terminal -> provider-neutral terminal/result staging transaction -> shutdown entire Job -> reread approved durable original Runtime evidence -> provider-neutral atomic terminal and Claim release.

Use existing Execution columns, no schema migration. Typed provider-neutral staged terminal mutation validates OCC, ownership, terminal and safe result and enters Finalizing without release. RuntimeTerminated supports existing Reconciling/Interrupted and Finalizing/exact staged terminal, with immutable staged result/completeness and approved original runtime evidence. SameRuntimeCleanup semantics unchanged.

Failed send never retries; mark dispatch/private uncertain where applicable. No reliable terminal means Interrupted with Unknown/Partial only after approved runtime evidence. Missing evidence means Unknown with Claim retained and staged terminal/result preserved. Startup reconciles exact staged/private terminal with same generic finalizer. Missing/contradictory private identity remains fail-closed.

Each direct execute freezes its own R1 identity before preparing. If concurrent preparation loses, it may only converge a binding/attempt that matches that R1; it cannot mark another caller's live Runtime Unknown or terminate it. This closes the bypass-admission ownership boundary without replacing Registry/TaskManager admission.

Observation is bounded and request-specific; SDK owns ids and waiter completion. Only full real pipe frame write + successful inner.flush emits observation. Neither timer nor response arrival proves dispatch. Caller drop must preserve owned lifecycle convergence.

Capabilities stay closed until gates pass. Windows target execute/activity/recover true; cancel/continue/token false; non-Windows false. Health/policy gate new admission only.
