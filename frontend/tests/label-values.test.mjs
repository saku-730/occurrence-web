import assert from "node:assert/strict";
import test from "node:test";
import { labelValuesFromNQuads } from "../src/lib/label-values.ts";

const graph = "<https://bio-database.net/graphs/occurrences>";
const dwc = "http://rs.tdwg.org/dwc/terms/";
const quad = (subject, predicate, object) => `<https://bio-database.net/occurrences/1/${subject}> <${predicate}> ${object} ${graph} .`;

test("collects event, location and arbitrary properties across component subjects", () => {
  const values = labelValuesFromNQuads([
    quad("events/1", dwc + "eventDate", '"2026-09-10"^^<http://www.w3.org/2001/XMLSchema#date>'),
    quad("locations/1", dwc + "locality", '"つくば市"@ja'),
    quad("identifications/1", dwc + "identifiedBy", '"Yamada"'),
  ].join("\n"));
  assert.deepEqual(values[dwc + "eventDate"], ["2026-09-10"]);
  assert.deepEqual(values[dwc + "locality"], ["つくば市"]);
  assert.deepEqual(values[dwc + "identifiedBy"], ["Yamada"]);
});

test("omits missing and empty values, retains distinct values, ignores blank node objects", () => {
  const predicate = dwc + "locality";
  const values = labelValuesFromNQuads([
    quad("locations/1", predicate, '"A"'),
    quad("locations/1", predicate, '"A"'),
    quad("locations/2", predicate, '"B"'),
    quad("locations/2", predicate, '"  "'),
    quad("locations/2", predicate, "_:internal"),
  ].join("\n"));
  assert.deepEqual(values[predicate], ["A", "B"]);
  assert.equal(values[dwc + "eventDate"], undefined);
});

test("decodes literal escapes once and preserves object IRIs without RDF syntax", () => {
  const values = labelValuesFromNQuads([
    quad("events/1", dwc + "eventRemarks", String.raw`"line\nquote \"x\" literal \\n \u65E5 \U0001F33F"`),
    quad("identifications/1", "http://rs.tdwg.org/dwc/iri/toTaxon", "<https://www.gbif.org/species/1>"),
  ].join("\n"));
  assert.deepEqual(values[dwc + "eventRemarks"], ['line\nquote "x" literal \\n 日 🌿']);
  assert.deepEqual(values["http://rs.tdwg.org/dwc/iri/toTaxon"], ["https://www.gbif.org/species/1"]);
});
