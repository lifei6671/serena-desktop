import { test } from 'node:test';
import assert from 'node:assert/strict';
import { agentRequests } from './agentRequests.ts';

test('transport retry retains lineage/key/prompt; explicit new operation starts a new lineage', () => {
  const request = agentRequests.fresh('frozen prompt');
  agentRequests.remember(request);
  const sent = JSON.stringify(request);
  // Ambiguous transport failure leaves the operation available to Retry.
  assert.equal(JSON.stringify(agentRequests.pending), sent);
  assert.equal(agentRequests.pending, request);
  const next = agentRequests.fresh('new conversation');
  assert.notEqual(next.agentId, request.agentId);
  assert.notEqual(next.requestKey, request.requestKey);
  assert.equal(JSON.stringify(agentRequests.pending), sent);
  agentRequests.accepted(request);
  assert.equal(agentRequests.pending, null);
});
test('continue carries only source identity and retry preserves the same key', () => {
  const request = agentRequests.continuation('E1', 'next prompt');
  agentRequests.remember(request);
  assert.equal('agentId' in request, false);
  assert.equal(request.executionId, 'E1');
  assert.equal(agentRequests.pending, request);
  agentRequests.accepted(request);
});
