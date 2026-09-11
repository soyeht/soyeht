import schema from './schema.json' with { type: 'json' };
import type { JsonValue, Methods } from './generated.ts';

interface Rule {
  $ref?: string;
  type?: string;
  enum?: JsonValue[];
  oneOf?: Rule[];
  properties?: Record<string, Rule>;
  required?: string[];
  items?: Rule;
  minimum?: number;
  maximum?: number;
  minLength?: number;
  maxLength?: number;
  pattern?: string;
  contentEncoding?: string;
}

const definitions: Record<string, Rule> = schema.$defs;

function valid(rule: Rule, value: JsonValue): boolean {
  if (rule.$ref) {
    const target = definitions[rule.$ref.slice('#/$defs/'.length)];
    if (!target) throw new Error('Unknown schema reference');
    return valid(target, value);
  }
  if (rule.oneOf) return rule.oneOf.filter((branch) => valid(branch, value)).length === 1;
  if (rule.enum && !rule.enum.includes(value)) return false;
  switch (rule.type) {
    case undefined: return true;
    case 'null': return value === null;
    case 'boolean': return typeof value === 'boolean';
    case 'string':
      if (typeof value !== 'string') return false;
      if (rule.minLength !== undefined && [...value].length < rule.minLength) return false;
      if (rule.maxLength !== undefined && [...value].length > rule.maxLength) return false;
      if (rule.pattern && !(new RegExp(rule.pattern)).test(value)) return false;
      if (rule.contentEncoding === 'base64' && !/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(value)) return false;
      return true;
    case 'number':
    case 'integer':
      if (typeof value !== 'number' || !Number.isFinite(value)) return false;
      if (rule.type === 'integer' && !Number.isSafeInteger(value)) return false;
      return (rule.minimum === undefined || value >= rule.minimum) && (rule.maximum === undefined || value <= rule.maximum);
    case 'array': return Array.isArray(value) && !!rule.items && value.every((item) => valid(rule.items!, item));
    case 'object':
      if (value === null || typeof value !== 'object' || Array.isArray(value)) return false;
      if (rule.required?.some((key) => !(key in value))) return false;
      return Object.entries(rule.properties ?? {}).every(([key, child]) => !(key in value) || valid(child, value[key]!));
    default: throw new Error('Unsupported schema type');
  }
}

export function decodeNamed(name: string, text: string): JsonValue {
  const rule = definitions[name];
  if (!rule) throw new Error('Unknown schema type');
  const value: JsonValue = JSON.parse(text);
  if (!valid(rule, value)) throw new Error(`Invalid ${name}`);
  return value;
}

export function decodeParams<K extends keyof Methods>(method: K, text: string): Methods[K]['params'] {
  const name = schema['x-methods'][method].params.$ref.slice('#/$defs/'.length);
  return decodeNamed(name, text) as Methods[K]['params'];
}

export function decodeResult<K extends keyof Methods>(method: K, text: string): Methods[K]['result'] {
  const name = schema['x-methods'][method].result.$ref.slice('#/$defs/'.length);
  return decodeNamed(name, text) as Methods[K]['result'];
}
