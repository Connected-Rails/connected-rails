// Reading a binary FBX, far enough to get the meshes out of it.
//
// FBX is Autodesk's, undocumented, and the usual way to read one is to run
// Blender. There is no Blender on the machines this repository is built on and
// no npm package in it, so the reader is here. The binary layout has been
// public knowledge for a decade and is simple enough to write down:
//
//   header      "Kaydara FBX Binary  \0" + two bytes + a uint32 version
//   record      end offset, property count, property bytes, name, properties,
//               then nested records until a run of zeroes as long as the
//               header of a record
//   property    a type letter and its payload; the array types carry a length,
//               an encoding and, where the encoding is 1, a deflate stream
//
// Offsets are 32 bit up to version 7500 and 64 bit from it, which is the one
// thing that silently reads a whole file as garbage if it is got wrong.
//
// What comes out is the record tree, unchanged. Turning that into meshes is
// `fbx_scene.mjs` — the two are separate because the tree is a fact about the
// file and the scene is an interpretation of it.

import { readFileSync } from 'node:fs';
import { inflateSync } from 'node:zlib';

const MAGIC = 'Kaydara FBX Binary  ';

/** One record: `{ name, properties, children }`. */
function readRecord(view, bytes, at, wide) {
  const endOffset = wide ? Number(view.getBigUint64(at, true)) : view.getUint32(at, true);
  if (endOffset === 0) return { end: at + (wide ? 25 : 13), record: null };
  let cursor = at + (wide ? 8 : 4);
  const count = wide ? Number(view.getBigUint64(cursor, true)) : view.getUint32(cursor, true);
  cursor += wide ? 8 : 4;
  // The length of the property block; it is implied by the properties
  // themselves, so it is read past rather than used.
  cursor += wide ? 8 : 4;
  const nameLength = view.getUint8(cursor);
  cursor += 1;
  const name = bytes.toString('latin1', cursor, cursor + nameLength);
  cursor += nameLength;

  const properties = [];
  for (let i = 0; i < count; i++) {
    const { value, next } = readProperty(view, bytes, cursor);
    properties.push(value);
    cursor = next;
  }

  const children = [];
  // Anything left before the record's own end is a nested record.
  while (cursor < endOffset) {
    const { end, record } = readRecord(view, bytes, cursor, wide);
    cursor = end;
    if (record) children.push(record);
    else break;
  }
  return { end: endOffset, record: { name, properties, children } };
}

/** Array properties: a length, an encoding, and the bytes. */
function readArray(view, bytes, at, size, read) {
  const length = view.getUint32(at, true);
  const encoding = view.getUint32(at + 4, true);
  const compressed = view.getUint32(at + 8, true);
  const from = at + 12;
  const raw =
    encoding === 1
      ? inflateSync(bytes.subarray(from, from + compressed))
      : bytes.subarray(from, from + length * size);
  const inner = new DataView(raw.buffer, raw.byteOffset, raw.byteLength);
  const out = new Array(length);
  for (let i = 0; i < length; i++) out[i] = read(inner, i * size);
  return { value: out, next: from + compressed };
}

function readProperty(view, bytes, at) {
  const type = String.fromCharCode(view.getUint8(at));
  const from = at + 1;
  switch (type) {
    case 'Y':
      return { value: view.getInt16(from, true), next: from + 2 };
    case 'C':
      return { value: view.getUint8(from) !== 0, next: from + 1 };
    case 'I':
      return { value: view.getInt32(from, true), next: from + 4 };
    case 'F':
      return { value: view.getFloat32(from, true), next: from + 4 };
    case 'D':
      return { value: view.getFloat64(from, true), next: from + 8 };
    case 'L':
      return { value: Number(view.getBigInt64(from, true)), next: from + 8 };
    case 'f':
      return readArray(view, bytes, from, 4, (v, o) => v.getFloat32(o, true));
    case 'd':
      return readArray(view, bytes, from, 8, (v, o) => v.getFloat64(o, true));
    case 'l':
      return readArray(view, bytes, from, 8, (v, o) => Number(v.getBigInt64(o, true)));
    case 'i':
      return readArray(view, bytes, from, 4, (v, o) => v.getInt32(o, true));
    case 'b':
      return readArray(view, bytes, from, 1, (v, o) => v.getUint8(o) !== 0);
    case 'S':
    case 'R': {
      const length = view.getUint32(from, true);
      const start = from + 4;
      const value =
        type === 'S'
          ? bytes.toString('latin1', start, start + length)
          : bytes.subarray(start, start + length);
      return { value, next: start + length };
    }
    default:
      throw new Error(`fbx: unknown property type '${type}' (0x${type.charCodeAt(0).toString(16)})`);
  }
}

/** The whole file as a record tree, plus its version. */
export function readFbx(path) {
  const bytes = readFileSync(path);
  if (bytes.toString('latin1', 0, MAGIC.length) !== MAGIC) {
    // An ASCII FBX starts with a comment or a keyword; it is a different
    // format altogether and is not read here.
    throw new Error(`${path}: not a binary FBX`);
  }
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const version = view.getUint32(23, true);
  const wide = version >= 7500;
  const children = [];
  let cursor = 27;
  while (cursor < bytes.length - 16) {
    const { end, record } = readRecord(view, bytes, cursor, wide);
    if (!record) break;
    children.push(record);
    cursor = end;
  }
  return { version, root: { name: '', properties: [], children } };
}

/** The first child of `record` with that name. */
export function child(record, name) {
  return record?.children.find((c) => c.name === name) ?? null;
}

/** Every child with that name. */
export function children(record, name) {
  return record?.children.filter((c) => c.name === name) ?? [];
}

/**
 * A value out of an FBX `Properties70` block.
 *
 * Every property is a `P` record whose first string is the name and whose
 * fifth onwards are the values — which is where a model's translation and a
 * material's colour live.
 */
export function property70(record, name) {
  const block = child(record, 'Properties70');
  if (!block) return null;
  for (const p of children(block, 'P')) {
    if (p.properties[0] === name) return p.properties.slice(4);
  }
  return null;
}
