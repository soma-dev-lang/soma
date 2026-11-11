/** A value that crosses the boundary. */
export type SomaValue = null | boolean | number | bigint | string | SomaValue[] | { [key: string]: SomaValue } | Variant;

/** A value of one of the program's sum types. */
export class Variant {
  constructor(type: string, variant: string, fields?: { [key: string]: SomaValue }, values?: SomaValue[]);
  readonly type: string;
  readonly variant: string;
  readonly fields: { [key: string]: SomaValue };
  readonly values: SomaValue[];
}

/** A handler raised: `kind` is the error's kind (`invariant`, `requires`, a `require … else Tag` tag, …). */
export class SomaError extends Error {
  readonly kind: string;
}

/** The program did not load: `errors` holds `soma check`'s diagnostics. */
export class LoadError extends Error {
  readonly errors: Array<{ message: string; code?: string; line?: number }>;
}

export interface HandlerInfo {
  params: Array<{ name: string; type: string }>;
  returns: string | null;
}

export interface Program {
  /** The `soma` version that runs the program. */
  readonly version: string;
  /** cell → handler → its typed parameters and result. */
  handlers(): { [cell: string]: { [handler: string]: HandlerInfo } };
  /** Call `handler` (or `Cell.handler`): arguments by position (an array) or by parameter name (an object). */
  call(handler: string, args?: SomaValue[] | { [param: string]: SomaValue }): Promise<SomaValue>;
  close(): Promise<void>;
  /** Handlers are methods too: `await app.refund("o1", 40)`. */
  [handler: string]: any;
}

/** Load a `.cell` program. `env`: extra environment for it (SOMA_LLM_MOCK, API keys…). */
export function load(path: string, options?: { env?: { [name: string]: string } }): Promise<Program>;
