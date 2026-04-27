// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import {
  gen_server_actor,
  event_actor,
  workflow_actor,
  handler,
  run_handler,
  signal_handler,
  query_handler,
  getActorDefinition,
} from '../dist/decorators.js';

function routeWorkflowMessage(actor, definition, msgType, payload) {
  if (msgType === 'workflow_run') {
    const method = definition.runHandler ?? 'run';
    return actor[method](payload);
  }
  if (msgType.startsWith('workflow_signal:')) {
    const name = msgType.slice('workflow_signal:'.length);
    const method = definition.signalHandlers[name] ?? 'signal';
    if (definition.signalHandlers[name]) {
      actor[method](payload);
    } else {
      actor[method](name, payload);
    }
    return {};
  }
  if (msgType.startsWith('workflow_query:')) {
    const name = msgType.slice('workflow_query:'.length);
    const method = definition.queryHandlers[name] ?? 'query';
    return definition.queryHandlers[name] ? actor[method](payload) : actor[method](name, payload);
  }
  return null;
}

describe('TypeScript abstraction contract', () => {
  it('captures aligned GenServer facets and handlers', () => {
    class AbstractionsActor {
      increment(payload) {
        return { count: payload.amount };
      }
      status() {
        return { status: 'ok' };
      }
    }

    handler('increment')(AbstractionsActor.prototype, 'increment', Object.getOwnPropertyDescriptor(AbstractionsActor.prototype, 'increment'));
    handler('status')(AbstractionsActor.prototype, 'status', Object.getOwnPropertyDescriptor(AbstractionsActor.prototype, 'status'));
    gen_server_actor({ facets: ['virtual_actor', 'durability', 'timer', 'reminder'] })(AbstractionsActor);

    const definition = getActorDefinition(AbstractionsActor);
    assert.ok(definition);
    assert.equal(definition.behaviorType, 'GenServer');
    assert.deepEqual(definition.facets, ['virtual_actor', 'durability', 'timer', 'reminder']);
    assert.equal(definition.handlers.increment.methodName, 'increment');
    assert.equal(definition.handlers.status.methodName, 'status');
  });

  it('captures workflow-specific handlers with canonical names', () => {
    class AbstractionsWorkflow {
      constructor() {
        this.status = 'pending';
        this.signals = [];
      }
      start(payload) {
        this.status = `running:${payload.orderId}`;
        return { status: this.status };
      }
      cancel(payload) {
        this.signals.push(payload.reason);
        this.status = 'cancelled';
      }
      currentStatus() {
        return { status: this.status, signals: this.signals };
      }
    }

    run_handler(AbstractionsWorkflow.prototype, 'start', Object.getOwnPropertyDescriptor(AbstractionsWorkflow.prototype, 'start'));
    signal_handler('cancel')(AbstractionsWorkflow.prototype, 'cancel', Object.getOwnPropertyDescriptor(AbstractionsWorkflow.prototype, 'cancel'));
    query_handler('status')(AbstractionsWorkflow.prototype, 'currentStatus', Object.getOwnPropertyDescriptor(AbstractionsWorkflow.prototype, 'currentStatus'));
    workflow_actor({ facets: ['virtual_actor', 'durability'] })(AbstractionsWorkflow);

    const definition = getActorDefinition(AbstractionsWorkflow);
    assert.ok(definition);
    assert.equal(definition.runHandler, 'start');
    assert.equal(definition.signalHandlers.cancel, 'cancel');
    assert.equal(definition.queryHandlers.status, 'currentStatus');

    const workflow = new AbstractionsWorkflow();
    assert.deepEqual(routeWorkflowMessage(workflow, definition, 'workflow_run', { orderId: 'o-1' }), { status: 'running:o-1' });
    assert.deepEqual(routeWorkflowMessage(workflow, definition, 'workflow_signal:cancel', { reason: 'user' }), {});
    assert.deepEqual(routeWorkflowMessage(workflow, definition, 'workflow_query:status', {}), { status: 'cancelled', signals: ['user'] });
  });

  it('models channel-style event handling with event_actor', () => {
    class AbstractionsChannel {
      constructor() {
        this.received = [];
      }

      publish(payload) {
        this.received.push({ channel: payload.channel, body: payload.body });
      }
    }

    handler('publish', 'cast')(AbstractionsChannel.prototype, 'publish', Object.getOwnPropertyDescriptor(AbstractionsChannel.prototype, 'publish'));
    event_actor({ facets: ['process_group'] })(AbstractionsChannel);

    const definition = getActorDefinition(AbstractionsChannel);
    assert.ok(definition);
    assert.equal(definition.behaviorType, 'GenEvent');
    assert.deepEqual(definition.facets, ['process_group']);
    assert.equal(definition.handlers.publish.invocation, 'cast');

    const channel = new AbstractionsChannel();
    channel.publish({ channel: 'alerts', body: 'hello' });
    assert.deepEqual(channel.received, [{ channel: 'alerts', body: 'hello' }]);
  });
});
