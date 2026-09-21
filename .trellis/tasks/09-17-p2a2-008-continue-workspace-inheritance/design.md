# Design — P2A2-008 Continue Workspace Inheritance

## Boundary

Continue is parent-record driven. Its only Workspace Authority is the persisted parent Execution selected by `parentExecutionId` / `executionId`. No Registry, Desktop selection, Global ActiveWorkspace, Session or last-request state participates.

## Contract

The Store builds the child canonical request from the parent record, preserving all three Workspace identity fields. Eligibility validates terminal/release conditions and complete identity before creation. Work linkage independently verifies parent membership and Work identity. A matching request key returns the original child.

## Test strategy

Reuse StateStore Work tests for creation, membership, claim and fail-closed ordering; reuse MCP orchestration tests for typed public and Local DTO rejection; extend restart evidence to assert all three inherited identity fields.

## Exclusions

No Thread/historyMode, Usage, Runtime Evidence, Claim release, Recovery, Cancellation, P2A2-009/010/012 or Capability Runtime changes.
