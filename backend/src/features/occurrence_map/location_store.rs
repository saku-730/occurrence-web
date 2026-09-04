use std::sync::Arc;

use oxrdf::{GraphName, NamedNode, NamedOrBlankNode, Quad, Term};
use oxrdfio::{RdfFormat, RdfParser, RdfSerializer};

use crate::features::occurrences::service::{
    DarwinCoreTerm, OccurrenceRdfStore, OccurrenceServiceError, PredicateObjectMapping,
    SearchOccurrencesStoreInput, SearchOccurrencesStorePage,
};

const OCCURRENCE_GRAPH_URI: &str = "https://bio-database.net/graphs/occurrences";
const CREATED_PREDICATE_URI: &str = "http://purl.org/dc/terms/created";
const HAS_LOCATION_PREDICATE_URI: &str = "https://bio-database.net/terms/hasLocation";
const RDF_TYPE_PREDICATE_URI: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const LOCATION_CLASS_URI: &str = "http://purl.org/dc/terms/Location";

const LOCATION_PREDICATE_URIS: &[&str] = &[
    "http://rs.tdwg.org/dwc/terms/decimalLatitude",
    "http://rs.tdwg.org/dwc/terms/decimalLongitude",
    "http://rs.tdwg.org/dwc/terms/geodeticDatum",
    "http://rs.tdwg.org/dwc/terms/coordinateUncertaintyInMeters",
    "http://rs.tdwg.org/dwc/terms/locality",
    "http://rs.tdwg.org/dwc/terms/verbatimLocality",
    "http://rs.tdwg.org/dwc/terms/island",
    "http://rs.tdwg.org/dwc/terms/islandGroup",
    "http://rs.tdwg.org/dwc/terms/waterBody",
    "http://rs.tdwg.org/dwc/terms/municipality",
    "http://rs.tdwg.org/dwc/terms/county",
    "http://rs.tdwg.org/dwc/terms/stateProvince",
    "http://rs.tdwg.org/dwc/terms/country",
    "http://rs.tdwg.org/dwc/iri/georeferenceSources",
    "http://rs.tdwg.org/dwc/terms/georeferenceProtocol",
    "http://rs.tdwg.org/dwc/terms/georeferencedDate",
    "http://rs.tdwg.org/dwc/terms/georeferenceRemarks",
];

/// Production decorator around the existing RDF store. The current occurrence service
/// already routes a subset of Darwin Core Location predicates. This decorator extends
/// that routing without changing the established CRUD service implementation.
pub struct ExtendedLocationRdfStore {
    inner: Arc<dyn OccurrenceRdfStore>,
}

impl ExtendedLocationRdfStore {
    pub fn new(inner: Arc<dyn OccurrenceRdfStore>) -> Self {
        Self { inner }
    }
}

#[async_trait::async_trait]
impl OccurrenceRdfStore for ExtendedLocationRdfStore {
    async fn save_nquads(&self, nquads: Vec<u8>) -> Result<(), OccurrenceServiceError> {
        self.inner
            .save_nquads(normalize_extended_location_nquads(&nquads)?)
            .await
    }

    async fn get_occurrence_nquads(
        &self,
        occurrence_uri: &str,
    ) -> Result<Option<Vec<u8>>, OccurrenceServiceError> {
        self.inner.get_occurrence_nquads(occurrence_uri).await
    }

    async fn is_media_referenced_by_public_occurrence(
        &self,
        media_uri: &str,
    ) -> Result<bool, OccurrenceServiceError> {
        self.inner
            .is_media_referenced_by_public_occurrence(media_uri)
            .await
    }

    async fn is_media_referenced_by_occurrence(
        &self,
        media_uri: &str,
    ) -> Result<bool, OccurrenceServiceError> {
        self.inner
            .is_media_referenced_by_occurrence(media_uri)
            .await
    }

    async fn replace_occurrence_nquads(
        &self,
        occurrence_uri: &str,
        nquads: Vec<u8>,
    ) -> Result<(), OccurrenceServiceError> {
        self.inner
            .replace_occurrence_nquads(occurrence_uri, normalize_extended_location_nquads(&nquads)?)
            .await
    }

    async fn delete_occurrence_nquads(
        &self,
        occurrence_uri: &str,
    ) -> Result<(), OccurrenceServiceError> {
        self.inner.delete_occurrence_nquads(occurrence_uri).await
    }

    async fn list_darwin_core_terms(&self) -> Result<Vec<DarwinCoreTerm>, OccurrenceServiceError> {
        self.inner.list_darwin_core_terms().await
    }

    async fn predicate_object_mapping(
        &self,
        predicate_uri: &str,
    ) -> Result<Option<PredicateObjectMapping>, OccurrenceServiceError> {
        self.inner.predicate_object_mapping(predicate_uri).await
    }

    async fn search_occurrences(
        &self,
        input: SearchOccurrencesStoreInput,
    ) -> Result<SearchOccurrencesStorePage, OccurrenceServiceError> {
        self.inner.search_occurrences(input).await
    }
}

fn normalize_extended_location_nquads(nquads: &[u8]) -> Result<Vec<u8>, OccurrenceServiceError> {
    let mut quads = RdfParser::from_format(RdfFormat::NQuads)
        .for_slice(nquads)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| OccurrenceServiceError::StoreFailed)?;

    let occurrence_uri = quads
        .iter()
        .find_map(|quad| {
            if quad.predicate.as_str() != CREATED_PREDICATE_URI {
                return None;
            }
            match &quad.subject {
                NamedOrBlankNode::NamedNode(subject) => Some(subject.as_str().to_string()),
                NamedOrBlankNode::BlankNode(_) => None,
            }
        })
        .ok_or(OccurrenceServiceError::StoreFailed)?;
    let location_uri = format!("{occurrence_uri}/locations/1");
    let location_subject =
        NamedNode::new(location_uri.clone()).map_err(|_| OccurrenceServiceError::StoreFailed)?;

    let mut has_location_value = false;
    for quad in &mut quads {
        if LOCATION_PREDICATE_URIS.contains(&quad.predicate.as_str()) {
            quad.subject = location_subject.clone().into();
            has_location_value = true;
        }
    }

    if has_location_value {
        let graph = NamedNode::new(OCCURRENCE_GRAPH_URI)
            .map_err(|_| OccurrenceServiceError::StoreFailed)?;
        let occurrence =
            NamedNode::new(occurrence_uri).map_err(|_| OccurrenceServiceError::StoreFailed)?;

        if !quads.iter().any(|quad| {
            quad.predicate.as_str() == HAS_LOCATION_PREDICATE_URI
                && matches!(&quad.object, Term::NamedNode(node) if node.as_str() == location_uri)
        }) {
            quads.push(Quad::new(
                occurrence,
                NamedNode::new(HAS_LOCATION_PREDICATE_URI)
                    .map_err(|_| OccurrenceServiceError::StoreFailed)?,
                location_subject.clone(),
                GraphName::NamedNode(graph.clone()),
            ));
        }

        if !quads.iter().any(|quad| {
            quad.subject == NamedOrBlankNode::NamedNode(location_subject.clone())
                && quad.predicate.as_str() == RDF_TYPE_PREDICATE_URI
                && matches!(&quad.object, Term::NamedNode(node) if node.as_str() == LOCATION_CLASS_URI)
        }) {
            quads.push(Quad::new(
                location_subject,
                NamedNode::new(RDF_TYPE_PREDICATE_URI)
                    .map_err(|_| OccurrenceServiceError::StoreFailed)?,
                NamedNode::new(LOCATION_CLASS_URI)
                    .map_err(|_| OccurrenceServiceError::StoreFailed)?,
                GraphName::NamedNode(graph),
            ));
        }
    }

    let mut serializer = RdfSerializer::from_format(RdfFormat::NQuads).for_writer(Vec::new());
    for quad in &quads {
        serializer
            .serialize_quad(quad)
            .map_err(|_| OccurrenceServiceError::StoreFailed)?;
    }
    serializer
        .finish()
        .map_err(|_| OccurrenceServiceError::StoreFailed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_extended_location_and_nominatim_provenance_to_location_node() {
        let occurrence =
            "https://bio-database.net/occurrences/550e8400-e29b-41d4-a716-446655440000";
        let input = format!(
            r#"<{occurrence}> <http://purl.org/dc/terms/created> "2026-09-02T00:00:00Z"^^<http://www.w3.org/2001/XMLSchema#dateTime> <{OCCURRENCE_GRAPH_URI}> .
<{occurrence}> <http://rs.tdwg.org/dwc/terms/stateProvince> "Kyoto" <{OCCURRENCE_GRAPH_URI}> .
<{occurrence}> <http://rs.tdwg.org/dwc/iri/georeferenceSources> <https://nominatim.openstreetmap.org/> <{OCCURRENCE_GRAPH_URI}> ."#
        );

        let output = normalize_extended_location_nquads(input.as_bytes()).unwrap();
        let text = String::from_utf8(output).unwrap();
        let location = format!("{occurrence}/locations/1");

        assert!(text.contains(&format!(
            "<{location}> <http://rs.tdwg.org/dwc/terms/stateProvince> \"Kyoto\""
        )));
        assert!(text.contains(&format!(
            "<{location}> <http://rs.tdwg.org/dwc/iri/georeferenceSources> <https://nominatim.openstreetmap.org/>"
        )));
        assert!(text.contains(&format!(
            "<{occurrence}> <{HAS_LOCATION_PREDICATE_URI}> <{location}>"
        )));
    }
}
