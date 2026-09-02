use oxrdf::Term;
use oxrdfio::{RdfFormat, RdfParser};

use crate::features::occurrences::service::{
    OccurrenceRdfStore, OccurrenceServiceError, SearchOccurrencesStoreInput, SearchVisibility,
};

use super::{
    dto::{
        OccurrenceMapFeature, OccurrenceMapFeatureCollection, OccurrenceMapGeometry,
        OccurrenceMapProperties,
    },
    geocoding::NOMINATIM_SOURCE_URI,
};

const MAP_PAGE_SIZE: u32 = 500;
const DECIMAL_LATITUDE: &str = "http://rs.tdwg.org/dwc/terms/decimalLatitude";
const DECIMAL_LONGITUDE: &str = "http://rs.tdwg.org/dwc/terms/decimalLongitude";
const EVENT_DATE: &str = "http://rs.tdwg.org/dwc/terms/eventDate";
const LOCALITY: &str = "http://rs.tdwg.org/dwc/terms/locality";
const MUNICIPALITY: &str = "http://rs.tdwg.org/dwc/terms/municipality";
const COUNTY: &str = "http://rs.tdwg.org/dwc/terms/county";
const STATE_PROVINCE: &str = "http://rs.tdwg.org/dwc/terms/stateProvince";
const COUNTRY: &str = "http://rs.tdwg.org/dwc/terms/country";
const GEOREFERENCE_SOURCES: &str = "http://rs.tdwg.org/dwc/iri/georeferenceSources";

pub async fn list_occurrence_map<S>(
    store: &S,
    visibility: SearchVisibility,
) -> Result<OccurrenceMapFeatureCollection, OccurrenceServiceError>
where
    S: OccurrenceRdfStore + ?Sized,
{
    let mut features = Vec::new();
    let mut cursor = None;

    loop {
        let page = store
            .search_occurrences(SearchOccurrencesStoreInput {
                filters: Vec::new(),
                limit: MAP_PAGE_SIZE,
                cursor: cursor.clone(),
                visibility: visibility.clone(),
            })
            .await?;

        for row in page.rows {
            let Some(nquads) = store.get_occurrence_nquads(&row.occurrence_uri).await? else {
                continue;
            };
            if let Some(feature) = feature_from_nquads(
                row.occurrence_id.to_string(),
                row.occurrence_uri,
                row.scientific_name,
                &nquads,
            )? {
                features.push(feature);
            }
        }

        if !page.has_next {
            break;
        }
        let Some(next_cursor) = page.next_cursor else {
            return Err(OccurrenceServiceError::StoreFailed);
        };
        if cursor.as_deref() == Some(next_cursor.as_str()) {
            return Err(OccurrenceServiceError::StoreFailed);
        }
        cursor = Some(next_cursor);
    }

    Ok(OccurrenceMapFeatureCollection {
        kind: "FeatureCollection".to_string(),
        features,
    })
}

fn feature_from_nquads(
    occurrence_id: String,
    occurrence_uri: String,
    scientific_name: Option<String>,
    nquads: &[u8],
) -> Result<Option<OccurrenceMapFeature>, OccurrenceServiceError> {
    let quads = RdfParser::from_format(RdfFormat::NQuads)
        .for_slice(nquads)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| OccurrenceServiceError::StoreFailed)?;

    let Some(latitude) = first_literal(&quads, DECIMAL_LATITUDE).and_then(parse_latitude) else {
        return Ok(None);
    };
    let Some(longitude) = first_literal(&quads, DECIMAL_LONGITUDE).and_then(parse_longitude) else {
        return Ok(None);
    };

    let is_nominatim = quads.iter().any(|quad| {
        quad.predicate.as_str() == GEOREFERENCE_SOURCES
            && matches!(
                &quad.object,
                Term::NamedNode(source) if source.as_str() == NOMINATIM_SOURCE_URI
            )
    });

    Ok(Some(OccurrenceMapFeature {
        kind: "Feature".to_string(),
        id: occurrence_id.clone(),
        geometry: OccurrenceMapGeometry {
            kind: "Point".to_string(),
            coordinates: vec![longitude, latitude],
        },
        properties: OccurrenceMapProperties {
            occurrence_id,
            occurrence_uri,
            scientific_name,
            event_date: first_literal(&quads, EVENT_DATE),
            locality: first_literal(&quads, LOCALITY),
            municipality: first_literal(&quads, MUNICIPALITY),
            county: first_literal(&quads, COUNTY),
            state_province: first_literal(&quads, STATE_PROVINCE),
            country: first_literal(&quads, COUNTRY),
            coordinate_source: if is_nominatim {
                "nominatim".to_string()
            } else {
                "original".to_string()
            },
        },
    }))
}

fn first_literal(quads: &[oxrdf::Quad], predicate_uri: &str) -> Option<String> {
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

fn parse_latitude(value: String) -> Option<f64> {
    let value = value.parse::<f64>().ok()?;
    (-90.0..=90.0).contains(&value).then_some(value)
}

fn parse_longitude(value: String) -> Option<f64> {
    let value = value.parse::<f64>().ok()?;
    (-180.0..=180.0).contains(&value).then_some(value)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use uuid::Uuid;

    use crate::features::occurrences::service::{
        SearchOccurrenceStoreRow, SearchOccurrencesStorePage,
    };

    use super::*;

    #[derive(Clone)]
    struct FakeStore {
        rows: Vec<SearchOccurrenceStoreRow>,
        nquads: Arc<Mutex<HashMap<String, Vec<u8>>>>,
        requested_visibility: Arc<Mutex<Vec<SearchVisibility>>>,
    }

    #[async_trait::async_trait]
    impl OccurrenceRdfStore for FakeStore {
        async fn save_nquads(&self, _nquads: Vec<u8>) -> Result<(), OccurrenceServiceError> {
            Ok(())
        }

        async fn get_occurrence_nquads(
            &self,
            occurrence_uri: &str,
        ) -> Result<Option<Vec<u8>>, OccurrenceServiceError> {
            Ok(self.nquads.lock().unwrap().get(occurrence_uri).cloned())
        }

        async fn search_occurrences(
            &self,
            input: SearchOccurrencesStoreInput,
        ) -> Result<SearchOccurrencesStorePage, OccurrenceServiceError> {
            self.requested_visibility
                .lock()
                .unwrap()
                .push(input.visibility);
            Ok(SearchOccurrencesStorePage {
                rows: self.rows.clone(),
                limit: input.limit,
                next_cursor: None,
                has_next: false,
            })
        }
    }

    fn row(id: Uuid) -> SearchOccurrenceStoreRow {
        SearchOccurrenceStoreRow {
            occurrence_id: id,
            occurrence_uri: format!("https://bio-database.net/occurrences/{id}"),
            creator_user_id: None,
            scientific_name: Some("Pheretima hilgendorfi".to_string()),
            basis_of_record: None,
            recorded_by: None,
            created: Some("2026-09-02T00:00:00Z".to_string()),
            modified: None,
            access_rights: None,
        }
    }

    #[tokio::test]
    async fn geojson_uses_longitude_latitude_order_and_exact_nominatim_source() {
        let id = Uuid::new_v4();
        let row = row(id);
        let nquads = format!(
            r#"<{0}/locations/1> <{1}> "35.0116" <https://bio-database.net/graphs/occurrences> .
<{0}/locations/1> <{2}> "135.7681" <https://bio-database.net/graphs/occurrences> .
<{0}/locations/1> <{3}> "Kyoto City" <https://bio-database.net/graphs/occurrences> .
<{0}/locations/1> <{4}> <{5}> <https://bio-database.net/graphs/occurrences> ."#,
            row.occurrence_uri,
            DECIMAL_LATITUDE,
            DECIMAL_LONGITUDE,
            LOCALITY,
            GEOREFERENCE_SOURCES,
            NOMINATIM_SOURCE_URI,
        );
        let store = FakeStore {
            rows: vec![row.clone()],
            nquads: Arc::new(Mutex::new(HashMap::from([(
                row.occurrence_uri.clone(),
                nquads.into_bytes(),
            )]))),
            requested_visibility: Arc::new(Mutex::new(Vec::new())),
        };

        let map = list_occurrence_map(&store, SearchVisibility::PublicOnly)
            .await
            .unwrap();

        assert_eq!(map.features.len(), 1);
        assert_eq!(map.features[0].geometry.coordinates, vec![135.7681, 35.0116]);
        assert_eq!(map.features[0].properties.coordinate_source, "nominatim");
        assert_eq!(map.features[0].properties.locality.as_deref(), Some("Kyoto City"));
        assert_eq!(
            store.requested_visibility.lock().unwrap().as_slice(),
            &[SearchVisibility::PublicOnly]
        );
    }

    #[tokio::test]
    async fn non_nominatim_source_is_original_and_incomplete_coordinates_are_omitted() {
        let complete_id = Uuid::new_v4();
        let incomplete_id = Uuid::new_v4();
        let complete = row(complete_id);
        let incomplete = row(incomplete_id);
        let complete_nquads = format!(
            r#"<{0}/locations/1> <{1}> "35" <https://bio-database.net/graphs/occurrences> .
<{0}/locations/1> <{2}> "135" <https://bio-database.net/graphs/occurrences> .
<{0}/locations/1> <{3}> <https://example.org/geocoder> <https://bio-database.net/graphs/occurrences> ."#,
            complete.occurrence_uri,
            DECIMAL_LATITUDE,
            DECIMAL_LONGITUDE,
            GEOREFERENCE_SOURCES,
        );
        let incomplete_nquads = format!(
            "<{}/locations/1> <{}> \"35\" <https://bio-database.net/graphs/occurrences> .",
            incomplete.occurrence_uri, DECIMAL_LATITUDE
        );
        let store = FakeStore {
            rows: vec![complete.clone(), incomplete.clone()],
            nquads: Arc::new(Mutex::new(HashMap::from([
                (complete.occurrence_uri.clone(), complete_nquads.into_bytes()),
                (incomplete.occurrence_uri.clone(), incomplete_nquads.into_bytes()),
            ]))),
            requested_visibility: Arc::new(Mutex::new(Vec::new())),
        };

        let map = list_occurrence_map(&store, SearchVisibility::All)
            .await
            .unwrap();

        assert_eq!(map.features.len(), 1);
        assert_eq!(map.features[0].id, complete_id.to_string());
        assert_eq!(map.features[0].properties.coordinate_source, "original");
    }
}
