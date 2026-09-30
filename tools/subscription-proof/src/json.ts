// JSON value type shared by the proof modules.
export type JsonValue = string | number | boolean | null | JsonValue[] | { [k: string]: JsonValue };
