// The detail API serializes one N-Quad per line. Keep all component subjects:
// eventDate belongs to Event and locality belongs to Location.
export function labelValuesFromNQuads(nquads: string): Record<string, string[]> {
  const values = new Map<string, string[]>();
  for (const line of nquads.split("\n")) {
    const quad = line.trim().match(/^(?:<[^>]+>|_:\S+)\s+<([^>]+)>\s+(.+)\s+<[^>]+>\s*\.$/u);
    if (!quad) continue;
    const object = quad[2].trim();
    const literal = object.match(/^"((?:\\.|[^"\\])*)"(?:@[a-zA-Z0-9-]+|\^\^<[^>]+>)?$/u);
    const iri = object.match(/^<([^>]+)>$/u);
    const value = literal ? decodeLiteral(literal[1]) : iri?.[1];
    if (value === undefined || value.trim() === "") continue;
    const existing = values.get(quad[1]) ?? [];
    if (!existing.includes(value)) existing.push(value);
    values.set(quad[1], existing);
  }
  return Object.fromEntries(values);
}

function decodeLiteral(value: string): string {
  // Decode in one pass so an escaped backslash followed by "n" stays literal.
  return value.replace(/\\(U[0-9a-fA-F]{8}|u[0-9a-fA-F]{4}|[tbnrf"'\\])/gu, (escaped, code: string) => {
    if (code.startsWith("u") || code.startsWith("U")) {
      const point = Number.parseInt(code.slice(1), 16);
      return point <= 0x10ffff ? String.fromCodePoint(point) : escaped;
    }
    return ({ t: "\t", b: "\b", n: "\n", r: "\r", f: "\f", '"': '"', "'": "'", "\\": "\\" } as Record<string, string>)[code];
  });
}
