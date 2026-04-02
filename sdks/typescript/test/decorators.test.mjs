// SPDX-License-Identifier: LGPL-2.1-or-later

import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import {
  actor,
  workflow_actor,
  handler,
  getActorDefinition,
} from '../dist/decorators.js';

describe('TypeScript actor decorators', () => {
  it('captures GenServer metadata and handler mappings', () => {
    class Counter {
      increment(_payload) {
        return { ok: true };
      }
    }

    handler('increment')(Counter.prototype, 'increment', Object.getOwnPropertyDescriptor(Counter.prototype, 'increment'));
    actor({ facets: ['virtual_actor', 'durability'] })(Counter);

    const definition = getActorDefinition(Counter);
    assert.ok(definition);
    assert.equal(definition.behaviorType, 'GenServer');
    assert.deepEqual(definition.facets, ['virtual_actor', 'durability']);
    assert.equal(definition.handlers.increment.methodName, 'increment');
  });

  it('captures workflow metadata', () => {
    class OrderWorkflow {}
    workflow_actor({ facets: ['durability'] })(OrderWorkflow);

    const definition = getActorDefinition(OrderWorkflow);
    assert.ok(definition);
    assert.equal(definition.behaviorType, 'Workflow');
    assert.deepEqual(definition.facets, ['durability']);
  });
});
