export function* entries(value: any, optionName: string): Generator<string> {
  if (typeof value === "string") {
    throw new TypeError(`${optionName} must not be a bare string`);
  }
  if (Array.isArray(value)) {
    for (const v of value) {
      if (typeof v === "string") yield v;
      else yield `${v[0]}=${v[1]}`;
    }
  } else if (value && typeof value === "object") {
    for (const [k, v] of Object.entries(value)) {
      yield `${k}=${v}`;
    }
  }
}
