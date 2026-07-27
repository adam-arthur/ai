import type { ZodType } from "zod";

import type { InvocationError } from "./error.ts";
import { Access, Internet } from "./runtime.ts";

export interface NodeOptions<I, O> {
  readonly name: string;
  readonly input: ZodType<I>;
  readonly output: ZodType<O>;
  readonly prompt: string;
  readonly access?: Access;
  readonly internet?: boolean;
}

export interface NodeSpec<I, O> {
  readonly name: string;
  readonly prompt: string;
  readonly access: Access;
  readonly internet: Internet;
  readonly inputSchema: ZodType<I>;
  readonly outputSchema: ZodType<O>;
}

/** A named agent operation with schema-validated input and output. */
export class Node<I, O> {
  /** @internal */
  readonly _spec: NodeSpec<I, O>;

  /** @internal */
  constructor(options: NodeOptions<I, O>) {
    this._spec = Object.freeze({
      name: options.name,
      prompt: options.prompt,
      access: options.access ?? Access.ReadOnly,
      internet: options.internet === true ? Internet.Enabled : Internet.Disabled,
      inputSchema: options.input,
      outputSchema: options.output,
    });
  }

  get name(): string {
    return this._spec.name;
  }

  /** Creates a request for this node without executing it. */
  withInput(input: I): NodeInvocation<I, O> {
    return new NodeInvocation(this, input);
  }
}

/** Creates a typed agent node with read-only, offline defaults. */
export function node<I, O>(options: NodeOptions<I, O>): Node<I, O> {
  return new Node(options);
}

/** A typed request to invoke a particular node. */
export class NodeInvocation<I, O> {
  readonly node: Node<I, O>;
  readonly input: I;

  /** @internal */
  constructor(node: Node<I, O>, input: I) {
    this.node = node;
    this.input = input;
  }
}

export type NodeOutcome<I, O> =
  | { readonly ok: true; readonly value: O }
  | {
      readonly ok: false;
      readonly error: InvocationError;
      readonly input: I;
      readonly invocation: number;
    };
