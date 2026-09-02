use std::collections::HashSet;

use axum::{
    body::{Body, to_bytes},
    extract::Request,
    http::{Method, StatusCode, header::CONTENT_LENGTH},
    middleware::Next,
    response::{IntoResponse, Response},
};
use chrono::Utc;
use oxrdf::{Literal, NamedNode, Quad, Term, vocab::xsd};
use oxrdfio::{RdfFormat, RdfParser, RdfSerializer};
use serde_json::Value;

use crate::infrastructure::nominatim::NominatimClient;

const MAX_INTERCEPTED_BODY_BYTES: usize = 2 * 1024 * 1024;
const DECIMAL_LATITUDE: &str = "http://rs.tdwg.org/dwc/terms/decimalLatitude";
const DECIMAL_LONGITUDE: &str = "http://rs.tdwg.org/dwc/terms/decimalLongitude";
const LOCALITY: &str = "http://rs.tdwg.org/dwc/terms/locality";
const VERBATIM_LOCALITY: &str = "http://rs.tdwg.org/dwc/terms/verbatimLocality";
const ISLAND: &str = "http://rs.tdwg.org/dwc/terms/island";
const ISLAND_GROUP: &str = "http://rs.tdwg.org/dwc/terms/islandGroup";
const WATER_BODY: &str = "http://rs.tdwg.org/dwc/terms/waterBody";
const MUNICIPALITY: &str = "http://rs.tdwg.org/dwc/terms/municipality";
const COUNTY: &str = "http://rs.tdwg.org/dwc/terms/county";
const STATE_PROVINCE: &str = "http://rs.tdwg.org/dwc/terms/stateProvince";
const COUNTRY: &str = "http://rs.tdwg.org/dwc/terms/country";
const GEOREFERENCE_SOURCES: &str = "http://rs.tdwg.org/dwc/iri/georeferenceSources";
const GEOREFERENCE_PROTOCOL: &str = "http://rs.tdwg.org/dwc/terms/georeferenceProtocol";
const GEOREFERENCED_DATE: &str = "http://rs.tdwg.org/dwc/terms/georeferencedDate";
pub const NOMINATIM_SOURCE_URI: &str = "https://nominatim.openstreetmap.org/";
const NOMINATIM_PROTOCOL: &str = "Nominatim search; first-ranked result selected automatically";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeocodedLocation {
    pub latitude: String,
    pub longitude: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocationGeocoderError {
    RequestFailed,
}

#[async_trait::async_trait]
pub trait LocationGeocoder: Send + Sync {
    async fn geocode(
        &self,
        query: &str,
    ) -> Result<Option<GeocodedLocation>, LocationGeocoderError>;
}

/// Apply Nominatim geocoding immediately before the existing occurrence handlers run.
/// Geocoder failures are deliberately fail-open: the original request proceeds and the
/// occurrence is stored without generated coordinates.
pub async fn geocoding_middleware(request: Request, next: Next) -> Response {
    let path = request.uri().path().to_string();
    let method = request.method().clone();
    let is_single = method == Method::POST
        && (path == "/occurrences"
            || (path.starts_with("/papers/") && path.ends_with("/occurrences")));
    let is_batch = method == Method::POST
        && path.starts_with("/papers/")
        && path.ends_with("/occurrences/batch");

    if !is_single && !is_batch {
        return next.run(request).await;
    }

    let (mut parts, body) = request.into_parts();
    let original = match to_bytes(body, MAX_INTERCEPTED_BODY_BYTES).await {
        Ok(bytes) => bytes,
        Err(_) => return StatusCode::PAYLOAD_TOO_LARGE.into_response(),
    };

    let rewritten = if is_batch {
        geocode_batch_json(&original, NominatimClient::global()).await
    } else {
        match enrich_nquads_with_geocoding(&original, NominatimClient::global()).await {
            Ok(bytes) => bytes,
            Err(_) => original.to_vec(),
        }
    };

    // The body can grow after coordinates/provenance are added, so a stale Content-Length
    // must not be forwarded to the downstream extractor.
    parts.headers.remove(CONTENT_LENGTH);
    let request = Request::from_parts(parts, Body::from(rewritten));
    next.run(request).await
}

async fn geocode_batch_json(body: &[u8], geocoder: &dyn LocationGeocoder) -> Vec<u8> {
    let Ok(mut value) = serde_json::from_slice::<Value>(body) else {
        return body.to_vec();
    };
    let Some(occurrences) = value
        .get_mut("occurrences")
        .and_then(Value::as_array_mut)
    else {
        return body.to_vec();
    };

    for occurrence in occurrences {
        let Some(nquads) = occurrence.as_str() else {
            continue;
        };
        if let Ok(enriched) = enrich_nquads_with_geocoding(nquads.as_bytes(), geocoder).await {
            if let Ok(text) = String::from_utf8(enriched) {
                *occurrence = Value::String(text);
            }
        }
    }

    serde_json::to_vec(&value).unwrap_or_else(|_| body.to_vec())
}

pub async fn enrich_nquads_with_geocoding<G>(
    nquads: &[u8],
    geocoder: &G,
) -> Result<Vec<u8>, ()>
where
    G: LocationGeocoder + ?Sized,
{
    let mut quads = RdfParser::from_format(RdfFormat::NQuads)
        .for_slice(nquads)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ())?;

    if quads.is_empty() {
        return Ok(nquads.to_vec());
    }

    let has_latitude = quads
        .iter()
        .any(|quad| quad.predicate.as_str() == DECIMAL_LATITUDE);
    let has_longitude = quads
        .iter()
        .any(|quad| quad.predicate.as_str() == DECIMAL_LONGITUDE);

    // Complete source coordinates always win. Partial coordinates are not silently completed.
    if has_latitude || has_longitude {
        return Ok(nquads.to_vec());
    }

    let Some(query) = build_geocoding_query(&quads) else {
        return Ok(nquads.to_vec());
    };

    let Some(result) = geocoder.geocode(&query).await.unwrap_or(None) else {
        return Ok(nquads.to_vec());
    };

    let subject = quads[0].subject.clone();
    let graph_name = quads[0].graph_name.clone();
    let decimal_latitude = NamedNode::new(DECIMAL_LATITUDE).map_err(|_| ())?;
    let decimal_longitude = NamedNode::new(DECIMAL_LONGITUDE).map_err(|_| ())?;
    let georeference_sources = NamedNode::new(GEOREFERENCE_SOURCES).map_err(|_| ())?;
    let georeference_protocol = NamedNode::new(GEOREFERENCE_PROTOCOL).map_err(|_| ())?;
    let georeferenced_date = NamedNode::new(GEOREFERENCED_DATE).map_err(|_| ())?;
    let nominatim = NamedNode::new(NOMINATIM_SOURCE_URI).map_err(|_| ())?;

    quads.push(Quad::new(
        subject.clone(),
        decimal_latitude,
        Literal::new_typed_literal(result.latitude, xsd::DECIMAL),
        graph_name.clone(),
    ));
    quads.push(Quad::new(
        subject.clone(),
        decimal_longitude,
        Literal::new_typed_literal(result.longitude, xsd::DECIMAL),
        graph_name.clone(),
    ));
    quads.push(Quad::new(
        subject.clone(),
        georeference_sources,
        nominatim,
        graph_name.clone(),
    ));
    quads.push(Quad::new(
        subject.clone(),
        georeference_protocol,
        Literal::new_simple_literal(NOMINATIM_PROTOCOL),
        graph_name.clone(),
    ));
    quads.push(Quad::new(
        subject,
        georeferenced_date,
        Literal::new_typed_literal(Utc::now().date_naive().to_string(), xsd::DATE),
        graph_name,
    ));

    serialize_nquads(&quads)
}

fn build_geocoding_query(quads: &[Quad]) -> Option<String> {
    let locality = first_literal(quads, LOCALITY).or_else(|| first_literal(quads, VERBATIM_LOCALITY));
    let ordered = [
        locality,
        first_literal(quads, ISLAND),
        first_literal(quads, ISLAND_GROUP),
        first_literal(quads, WATER_BODY),
        first_literal(quads, MUNICIPALITY),
        first_literal(quads, COUNTY),
        first_literal(quads, STATE_PROVINCE),
        first_literal(quads, COUNTRY),
    ];

    let mut seen = HashSet::new();
    let mut parts = Vec::new();
    for value in ordered.into_iter().flatten() {
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        let key = value.to_lowercase();
        if seen.insert(key) {
            parts.push(value.to_string());
        }
    }

    (!parts.is_empty()).then(|| parts.join(", "))
}

fn first_literal(quads: &[Quad], predicate_uri: &str) -> Option<String> {
    quads.iter().find_map(|quad| {
        if quad.predicate.as_str() != predicate_uri {
            return None;
        }
        match &quad.object {
            Term::Literal(value) => Some(value.value().to_string()),
            _ => None,
        }
    })
}

fn serialize_nquads(quads: &[Quad]) -> Result<Vec<u8>, ()> {
    let mut serializer = RdfSerializer::from_format(RdfFormat::NQuads).for_writer(Vec::new());
    for quad in quads {
        serializer.serialize_quad(quad).map_err(|_| ())?;
    }
    serializer.finish().map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    #[derive(Clone)]
    struct FakeGeocoder {
        result: Result<Option<GeocodedLocation>, LocationGeocoderError>,
        queries: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait::async_trait]
    impl LocationGeocoder for FakeGeocoder {
        async fn geocode(
            &self,
            query: &str,
        ) -> Result<Option<GeocodedLocation>, LocationGeocoderError> {
            self.queries.lock().unwrap().push(query.to_string());
            self.result.clone()
        }
    }

    fn fake(result: Result<Option<GeocodedLocation>, LocationGeocoderError>) -> FakeGeocoder {
        FakeGeocoder {
            result,
            queries: Arc::new(Mutex::new(Vec::new())),
        }
    }

    #[test]
    fn locality_is_first_and_duplicate_hierarchy_values_are_removed() {
        let input = br#"_:o <http://rs.tdwg.org/dwc/terms/locality> "Arashiyama" <https://bio-database.net/graphs/occurrences> .
_:o <http://rs.tdwg.org/dwc/terms/municipality> "Kyoto" <https://bio-database.net/graphs/occurrences> .
_:o <http://rs.tdwg.org/dwc/terms/stateProvince> "kyoto" <https://bio-database.net/graphs/occurrences> .
_:o <http://rs.tdwg.org/dwc/terms/country> "Japan" <https://bio-database.net/graphs/occurrences> ."#;
        let quads = RdfParser::from_format(RdfFormat::NQuads)
            .for_slice(input)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(
            build_geocoding_query(&quads).as_deref(),
            Some("Arashiyama, Kyoto, Japan")
        );
    }

    #[test]
    fn verbatim_locality_is_used_only_when_locality_is_missing() {
        let input = br#"_:o <http://rs.tdwg.org/dwc/terms/verbatimLocality> "Old field locality" <https://bio-database.net/graphs/occurrences> .
_:o <http://rs.tdwg.org/dwc/terms/country> "Japan" <https://bio-database.net/graphs/occurrences> ."#;
        let quads = RdfParser::from_format(RdfFormat::NQuads)
            .for_slice(input)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(
            build_geocoding_query(&quads).as_deref(),
            Some("Old field locality, Japan")
        );
    }

    #[tokio::test]
    async fn complete_source_coordinates_skip_geocoder() {
        let input = br#"_:o <http://rs.tdwg.org/dwc/terms/decimalLatitude> "35.0" <https://bio-database.net/graphs/occurrences> .
_:o <http://rs.tdwg.org/dwc/terms/decimalLongitude> "135.0" <https://bio-database.net/graphs/occurrences> .
_:o <http://rs.tdwg.org/dwc/terms/locality> "Kyoto" <https://bio-database.net/graphs/occurrences> ."#;
        let geocoder = fake(Ok(Some(GeocodedLocation {
            latitude: "1".into(),
            longitude: "2".into(),
        })));

        let output = enrich_nquads_with_geocoding(input, &geocoder).await.unwrap();

        assert_eq!(output, input);
        assert!(geocoder.queries.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn generated_coordinates_and_nominatim_source_are_added() {
        let input = br#"_:o <http://rs.tdwg.org/dwc/terms/locality> "Kyoto City" <https://bio-database.net/graphs/occurrences> ."#;
        let geocoder = fake(Ok(Some(GeocodedLocation {
            latitude: "35.0116".into(),
            longitude: "135.7681".into(),
        })));

        let output = enrich_nquads_with_geocoding(input, &geocoder).await.unwrap();
        let text = String::from_utf8(output).unwrap();

        assert!(text.contains(DECIMAL_LATITUDE));
        assert!(text.contains("35.0116"));
        assert!(text.contains(DECIMAL_LONGITUDE));
        assert!(text.contains("135.7681"));
        assert!(text.contains(GEOREFERENCE_SOURCES));
        assert!(text.contains(NOMINATIM_SOURCE_URI));
        assert_eq!(geocoder.queries.lock().unwrap().as_slice(), &["Kyoto City"]);
    }

    #[tokio::test]
    async fn zero_result_or_geocoder_failure_keeps_original_occurrence() {
        let input = br#"_:o <http://rs.tdwg.org/dwc/terms/locality> "Unknown place" <https://bio-database.net/graphs/occurrences> ."#;

        for result in [Ok(None), Err(LocationGeocoderError::RequestFailed)] {
            let geocoder = fake(result);
            let output = enrich_nquads_with_geocoding(input, &geocoder).await.unwrap();
            assert_eq!(output, input);
        }
    }
}
