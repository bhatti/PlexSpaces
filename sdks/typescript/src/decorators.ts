// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors

export type BehaviorType = 'GenServer' | 'GenEvent' | 'GenStateMachine' | 'Workflow';
export type InvocationType = 'call' | 'cast';

export interface HandlerMetadata {
  methodName: string;
  msgTypes: string[];
  invocation: InvocationType;
}

export interface ActorDefinitionMetadata {
  behaviorType: BehaviorType;
  facets: string[];
  handlers: Record<string, HandlerMetadata>;
  runHandler?: string;
  signalHandlers: Record<string, string>;
  queryHandlers: Record<string, string>;
  fsmStates?: string[];
  fsmInitial?: string;
}

const ACTOR_METADATA = Symbol.for('plexspaces.actor.metadata');

function ensureMetadata(target: object): ActorDefinitionMetadata {
  const ctor = typeof target === 'function' ? target : (target as { constructor: object }).constructor;
  const existing = Reflect.get(ctor, ACTOR_METADATA) as ActorDefinitionMetadata | undefined;
  if (existing) return existing;
  const created: ActorDefinitionMetadata = {
    behaviorType: 'GenServer',
    facets: [],
    handlers: {},
    signalHandlers: {},
    queryHandlers: {},
  };
  Reflect.set(ctor, ACTOR_METADATA, created);
  return created;
}

function actorDecorator(behaviorType: BehaviorType, facets: string[] = []) {
  return function <T extends Function>(target: T): T {
    const metadata = ensureMetadata(target);
    metadata.behaviorType = behaviorType;
    metadata.facets = [...facets];
    return target;
  };
}

export function actor(options: { facets?: string[] } = {}) {
  return actorDecorator('GenServer', options.facets ?? []);
}

export function gen_server_actor(options: { facets?: string[] } = {}) {
  return actorDecorator('GenServer', options.facets ?? []);
}

export function event_actor(options: { facets?: string[] } = {}) {
  return actorDecorator('GenEvent', options.facets ?? []);
}

export function fsm_actor(options: { facets?: string[]; states?: string[]; initial?: string } = {}) {
  return function <T extends Function>(target: T): T {
    const result = actorDecorator('GenStateMachine', options.facets ?? [])(target);
    if (options.states !== undefined || options.initial !== undefined) {
      const metadata = ensureMetadata(result);
      if (options.states !== undefined) metadata.fsmStates = [...options.states];
      if (options.initial !== undefined) metadata.fsmInitial = options.initial;
    }
    return result;
  };
}

export function workflow_actor(options: { facets?: string[] } = {}) {
  return actorDecorator('Workflow', options.facets ?? []);
}

export function handler(...msgTypes: string[]) {
  let invocation: InvocationType = 'call';
  const effectiveTypes = msgTypes.filter((value) => {
    if (value === 'call' || value === 'cast') {
      invocation = value;
      return false;
    }
    return true;
  });

  return function (target: object, propertyKey: string, _descriptor: PropertyDescriptor): void {
    const metadata = ensureMetadata(target);
    const entry: HandlerMetadata = {
      methodName: propertyKey,
      msgTypes: effectiveTypes,
      invocation,
    };
    for (const msgType of effectiveTypes) {
      metadata.handlers[msgType] = entry;
    }
  };
}

export const init_handler = handler;

export function run_handler(target: object, propertyKey: string, _descriptor: PropertyDescriptor): void {
  const metadata = ensureMetadata(target);
  metadata.runHandler = propertyKey;
}

export function signal_handler(...names: string[]) {
  return function (target: object, propertyKey: string, _descriptor: PropertyDescriptor): void {
    const metadata = ensureMetadata(target);
    for (const name of names) {
      metadata.signalHandlers[name] = propertyKey;
    }
  };
}

export function query_handler(...names: string[]) {
  return function (target: object, propertyKey: string, _descriptor: PropertyDescriptor): void {
    const metadata = ensureMetadata(target);
    for (const name of names) {
      metadata.queryHandlers[name] = propertyKey;
    }
  };
}

export function getActorDefinition(target: object): ActorDefinitionMetadata | undefined {
  const ctor = typeof target === 'function' ? target : (target as { constructor: object }).constructor;
  return Reflect.get(ctor, ACTOR_METADATA) as ActorDefinitionMetadata | undefined;
}
