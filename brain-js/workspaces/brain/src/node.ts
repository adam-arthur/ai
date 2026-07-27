import type { ZodType } from "zod";

import { Access, Internet } from "./runtime.js";
import type { InvocationError } from "./error.js";

export interface NodeSpec<O> {
  readonly name: string;
  readonly prompt: string;
  readonly access: Access;
  readonly internet: Internet;
  readonly outputSchema: ZodType<O>;
}

/** A named agent operation with typed input and schema-validated output. */
export class Node<I, O> {
  /** @internal */
  readonly _spec: NodeSpec<O>;

  constructor(name: string, outputSchema: ZodType<O>, spec?: NodeSpec<O>) {
    this._spec =
      spec ??
      Object.freeze({
        name,
        prompt: "",
        access: Access.ReadOnly,
        internet: Internet.Disabled,
        outputSchema,
      });
  }

  prompt(prompt: string): Node<I, O> {
    return this.withSpec({ prompt });
  }

  access(access: Access): Node<I, O> {
    return this.withSpec({ access });
  }

  internet(internet: Internet): Node<I, O> {
    return this.withSpec({ internet });
  }

  get name(): string {
    return this._spec.name;
  }

  /** Returns another handle with the same identity, mirroring Rust's clone. */
  clone(): Node<I, O> {
    return new Node(this.name, this._spec.outputSchema, this._spec);
  }

  /** Creates one invocation without consuming the reusable node handle. */
  with(input: I): NodeInvocation<I, O> {
    return new NodeInvocation(this, input);
  }

  private withSpec(changes: Partial<NodeSpec<O>>): Node<I, O> {
    const spec = Object.freeze({ ...this._spec, ...changes });
    return new Node(spec.name, spec.outputSchema, spec);
  }
}

/** Creates a typed agent node with read-only, offline defaults. */
export function node<I, O>(name: string, outputSchema: ZodType<O>): Node<I, O> {
  return new Node(name, outputSchema);
}

/** A typed request to invoke a particular node. */
export class NodeInvocation<I, O> {
  /** @internal */
  constructor(
    readonly node: Node<I, O>,
    readonly input: I,
  ) {}
}

export type NodeOutcome<I, O> =
  | { readonly ok: true; readonly value: O }
  | { readonly ok: false; readonly failure: NodeFailure<I> };

/** A failed node invocation that retains its original input. */
export class NodeFailure<I> {
  readonly #input: I;
  readonly #error: InvocationError;
  readonly #invocation: number;

  /** @internal */
  constructor(input: I, error: InvocationError, invocation: number) {
    this.#input = input;
    this.#error = error;
    this.#invocation = invocation;
  }

  input(): I {
    return this.#input;
  }

  error(): InvocationError {
    return this.#error;
  }

  invocation(): number {
    return this.#invocation;
  }

  intoInput(): I {
    return this.#input;
  }

  intoError(): InvocationError {
    return this.#error;
  }

  intoParts(): readonly [I, InvocationError] {
    return [this.#input, this.#error] as const;
  }
}
