import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

function createShardGroup(rawCall, request) {
  const result = rawCall(JSON.stringify(request));
  if (result.startsWith('ERROR:')) {
    throw new Error(result);
  }
  return JSON.parse(result);
}

function applicationGetStatus(rawCall, applicationId, nodeId) {
  const result = rawCall(applicationId, nodeId);
  if (result.startsWith('ERROR:')) {
    throw new Error(result);
  }
  return JSON.parse(result);
}

describe('Host shard-group wrappers', () => {
  it('createShardGroup parses host JSON response', () => {
    const response = createShardGroup(
      () => JSON.stringify({
        group_id: 'group-a',
        actor_type: 'worker',
        shard_actor_ids: ['worker-0@test-node'],
      }),
      { group_id: 'group-a', actor_type: 'worker', shard_count: 1 },
    );
    assert.equal(response.group_id, 'group-a');
    assert.deepEqual(response.shard_actor_ids, ['worker-0@test-node']);
  });

  it('applicationGetStatus parses host JSON response', () => {
    const response = applicationGetStatus(
      () => JSON.stringify({
        node_id: 'node-a',
        application: { application_id: 'app-a' },
      }),
      'app-a',
      'node-a',
    );
    assert.equal(response.node_id, 'node-a');
    assert.equal(response.application.application_id, 'app-a');
  });
});
